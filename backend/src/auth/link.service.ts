import { randomBytes } from 'node:crypto';
import {
  BadRequestException,
  Injectable,
  Logger,
  NotFoundException,
  UnauthorizedException,
} from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { LessThan, Repository } from 'typeorm';
import { AuthService } from './auth.service.js';
import {
  LinkRequest,
  type LinkRequestMode,
  type LinkRequestStatus,
} from './entities/link-request.entity.js';
import { Session } from './entities/session.entity.js';

const LINK_REQUEST_TTL_MS = 5 * 60_000;

export interface CreateLinkResult {
  linkRequestId: string;
  claimToken: string;
  expiresAt: Date;
}

export interface LinkStatusResult {
  status: LinkRequestStatus | 'expired';
  mode: LinkRequestMode;
  /** Появляется когда status=claimed и mode=pull (receiver получает свой sessionId). */
  sessionId?: string;
  error?: string;
}

@Injectable()
export class LinkService {
  private readonly logger = new Logger(LinkService.name);

  constructor(
    @InjectRepository(LinkRequest)
    private readonly linkRepo: Repository<LinkRequest>,
    @InjectRepository(Session)
    private readonly sessionRepo: Repository<Session>,
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
      const exists = await this.sessionRepo.findOne({ where: { id: sourceSessionId } });
      if (!exists) throw new UnauthorizedException('Source session not found');
    }

    const claimToken = randomBytes(24).toString('base64url');
    const expiresAt = new Date(Date.now() + LINK_REQUEST_TTL_MS);

    const link = this.linkRepo.create({
      claimToken,
      mode,
      sourceSessionId: sourceSessionId ?? null,
      targetSessionId: null,
      status: 'pending',
      expiresAt,
    });
    await this.linkRepo.save(link);

    this.logger.log(`Link request created: id=${link.id} mode=${mode}`);
    return { linkRequestId: link.id, claimToken, expiresAt };
  }

  /**
   * Claim a link request by claim token.
   *
   * pull: вызывает source-устройство (передаёт свою сессию receiver-у).
   *   Требует sourceSessionId. Возвращает targetSessionId — id новой сессии для receiver.
   * push: вызывает receiver-устройство (забирает сессию у source).
   *   sourceSessionId игнорируется (берётся из LinkRequest). Возвращает sessionId для receiver.
   */
  async claim(
    claimToken: string,
    sourceSessionIdFromCaller?: string,
  ): Promise<{ sessionId: string; mode: LinkRequestMode }> {
    const link = await this.linkRepo.findOne({ where: { claimToken } });
    if (!link) throw new NotFoundException('Invalid or already used link token');

    if (link.status !== 'pending') {
      throw new BadRequestException('Link token is already used or expired');
    }
    if (link.expiresAt.getTime() < Date.now()) {
      link.status = 'failed';
      link.error = 'Expired';
      await this.linkRepo.save(link);
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

    const source = await this.sessionRepo.findOne({ where: { id: sourceSessionId } });
    if (!source) throw new UnauthorizedException('Source session not found');

    // Гарантируем что accessToken свежий (refresh если протух) перед копированием.
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
    const refreshedSource = await this.sessionRepo.findOne({ where: { id: source.id } });
    if (!refreshedSource) throw new UnauthorizedException('Source session not found');

    // Создаём НОВУЮ сессию-копию для target-устройства.
    // Не клонируем refreshToken: каждое устройство получает свой refreshToken после первого
    // успешного refresh? — но SoundCloud OAuth выдаёт один refresh_token для одного auth code.
    // Реалистично: копируем оба токена. SC поддерживает несколько concurrent сессий с одним
    // refresh_token (token rotation отключён или мягкий). Если после первого refresh старый
    // refresh_token инвалидируется — обе сессии просто получат 401 и пройдут re-auth.
    const target = this.sessionRepo.create({
      accessToken: refreshedSource.accessToken,
      refreshToken: refreshedSource.refreshToken,
      expiresAt: refreshedSource.expiresAt,
      scope: refreshedSource.scope,
      soundcloudUserId: refreshedSource.soundcloudUserId,
      username: refreshedSource.username,
      oauthAppId: refreshedSource.oauthAppId,
    });
    await this.sessionRepo.save(target);

    link.sourceSessionId = sourceSessionId;
    link.targetSessionId = target.id;
    link.status = 'claimed';
    await this.linkRepo.save(link);

    this.logger.log(
      `Link claimed: id=${link.id} mode=${link.mode} source=${sourceSessionId} target=${target.id}`,
    );
    return { sessionId: target.id, mode: link.mode };
  }

  async getStatus(linkRequestId: string): Promise<LinkStatusResult> {
    const link = await this.linkRepo.findOne({ where: { id: linkRequestId } });
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
      await this.linkRepo.delete({ expiresAt: LessThan(new Date()) });
    } catch (err: any) {
      this.logger.warn(`Failed to cleanup expired link requests: ${err?.message}`);
    }
  }
}
