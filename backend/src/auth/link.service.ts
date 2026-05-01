import { randomBytes } from 'node:crypto';
import {
  BadRequestException,
  Inject,
  Injectable,
  Logger,
  NotFoundException,
  UnauthorizedException,
} from '@nestjs/common';
import { eq, lt } from 'drizzle-orm';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { linkRequests, sessions } from '../db/schema.js';
import { AuthService } from './auth.service.js';

const LINK_REQUEST_TTL_MS = 5 * 60_000;

export type LinkRequestMode = 'pull' | 'push';
export type LinkRequestStatus = 'pending' | 'claimed' | 'failed';

export interface CreateLinkResult {
  linkRequestId: string;
  claimToken: string;
  expiresAt: Date;
}

export interface LinkStatusResult {
  status: LinkRequestStatus | 'expired';
  mode: LinkRequestMode;
  sessionId?: string;
  error?: string;
}

@Injectable()
export class LinkService {
  private readonly logger = new Logger(LinkService.name);

  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly authService: AuthService,
  ) {}

  async create(mode: LinkRequestMode, sourceSessionId?: string): Promise<CreateLinkResult> {
    void this.cleanup();

    if (mode === 'push' && !sourceSessionId) {
      throw new BadRequestException('push mode requires source session');
    }
    if (mode === 'pull' && sourceSessionId) {
      throw new BadRequestException('pull mode must not have source session at creation');
    }

    if (sourceSessionId) {
      const exists = await this.db.query.sessions.findFirst({
        where: eq(sessions.id, sourceSessionId),
      });
      if (!exists) throw new UnauthorizedException('Source session not found');
    }

    const claimToken = randomBytes(24).toString('base64url');
    const expiresAt = new Date(Date.now() + LINK_REQUEST_TTL_MS);

    const [link] = await this.db
      .insert(linkRequests)
      .values({
        claimToken,
        mode,
        sourceSessionId: sourceSessionId ?? null,
        targetSessionId: null,
        status: 'pending',
        expiresAt,
      })
      .returning();

    this.logger.log(`Link request created: id=${link.id} mode=${mode}`);
    return { linkRequestId: link.id, claimToken, expiresAt };
  }

  async claim(
    claimToken: string,
    sourceSessionIdFromCaller?: string,
  ): Promise<{ sessionId: string; mode: LinkRequestMode }> {
    const link = await this.db.query.linkRequests.findFirst({
      where: eq(linkRequests.claimToken, claimToken),
    });
    if (!link) throw new NotFoundException('Invalid or already used link token');

    if (link.status !== 'pending') {
      throw new BadRequestException('Link token is already used or expired');
    }
    if (link.expiresAt.getTime() < Date.now()) {
      await this.db
        .update(linkRequests)
        .set({ status: 'failed', error: 'Expired' })
        .where(eq(linkRequests.id, link.id));
      throw new BadRequestException('Link token expired');
    }

    let sourceSessionId: string;
    if (link.mode === 'pull') {
      if (!sourceSessionIdFromCaller) {
        throw new UnauthorizedException('pull claim requires source session');
      }
      sourceSessionId = sourceSessionIdFromCaller;
    } else {
      if (!link.sourceSessionId) {
        throw new BadRequestException('push link has no source session');
      }
      sourceSessionId = link.sourceSessionId;
    }

    const source = await this.db.query.sessions.findFirst({
      where: eq(sessions.id, sourceSessionId),
    });
    if (!source) throw new UnauthorizedException('Source session not found');

    try {
      await this.authService.getValidAccessToken(source.id);
    } catch (err: any) {
      this.logger.warn(
        `Failed to refresh source session ${source.id} before link: ${err?.message}`,
      );
      throw new UnauthorizedException(
        'Source session is not valid. The originating device must re-authenticate.',
      );
    }
    const refreshedSource = await this.db.query.sessions.findFirst({
      where: eq(sessions.id, source.id),
    });
    if (!refreshedSource) throw new UnauthorizedException('Source session not found');

    const [target] = await this.db
      .insert(sessions)
      .values({
        accessToken: refreshedSource.accessToken,
        refreshToken: refreshedSource.refreshToken,
        expiresAt: refreshedSource.expiresAt,
        scope: refreshedSource.scope,
        soundcloudUserId: refreshedSource.soundcloudUserId,
        username: refreshedSource.username,
        oauthAppId: refreshedSource.oauthAppId,
      })
      .returning();

    await this.db
      .update(linkRequests)
      .set({ sourceSessionId, targetSessionId: target.id, status: 'claimed' })
      .where(eq(linkRequests.id, link.id));

    this.logger.log(
      `Link claimed: id=${link.id} mode=${link.mode} source=${sourceSessionId} target=${target.id}`,
    );
    return { sessionId: target.id, mode: link.mode };
  }

  async getStatus(linkRequestId: string): Promise<LinkStatusResult> {
    const link = await this.db.query.linkRequests.findFirst({
      where: eq(linkRequests.id, linkRequestId),
    });
    if (!link) return { status: 'expired', mode: 'pull', error: 'Unknown link request' };

    if (link.status === 'pending' && link.expiresAt.getTime() < Date.now()) {
      return { status: 'expired', mode: link.mode, error: 'Expired' };
    }

    return {
      status: link.status,
      mode: link.mode,
      sessionId:
        link.mode === 'pull' && link.status === 'claimed'
          ? (link.targetSessionId ?? undefined)
          : undefined,
      error: link.error ?? undefined,
    };
  }

  private async cleanup(): Promise<void> {
    try {
      await this.db.delete(linkRequests).where(lt(linkRequests.expiresAt, new Date()));
    } catch (err: any) {
      this.logger.warn(`Failed to cleanup expired link requests: ${err?.message}`);
    }
  }
}
