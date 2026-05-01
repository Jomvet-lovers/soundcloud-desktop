import { Inject, Injectable, UnauthorizedException } from '@nestjs/common';
import { and, desc, eq, inArray, lt } from 'drizzle-orm';
import { AuthService } from '../auth/auth.service.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { localLikes } from '../db/schema.js';

@Injectable()
export class LocalLikesService {
  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly authService: AuthService,
  ) {}

  private async getScUserId(sessionId: string): Promise<string> {
    const session = await this.authService.getSession(sessionId);
    if (!session?.soundcloudUserId) {
      throw new UnauthorizedException('User not found in session');
    }
    return session.soundcloudUserId;
  }

  async add(
    sessionId: string,
    scTrackId: string,
    trackData: Record<string, unknown>,
  ): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    await this.db
      .insert(localLikes)
      .values({ soundcloudUserId, scTrackId, trackData })
      .onConflictDoNothing({ target: [localLikes.soundcloudUserId, localLikes.scTrackId] });
  }

  async remove(sessionId: string, scTrackId: string): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    await this.db
      .delete(localLikes)
      .where(
        and(eq(localLikes.soundcloudUserId, soundcloudUserId), eq(localLikes.scTrackId, scTrackId)),
      );
  }

  async findAll(
    sessionId: string,
    limit: number,
    cursor?: string,
  ): Promise<{ collection: Record<string, unknown>[]; next_href: string | null }> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    const conds = [eq(localLikes.soundcloudUserId, soundcloudUserId)];
    if (cursor) conds.push(lt(localLikes.createdAt, new Date(cursor)));

    const items = await this.db
      .select()
      .from(localLikes)
      .where(and(...conds))
      .orderBy(desc(localLikes.createdAt))
      .limit(limit + 1);

    const hasMore = items.length > limit;
    const collection = items.slice(0, limit);

    return {
      collection: collection.map((item) => item.trackData),
      next_href: hasMore
        ? `?limit=${limit}&cursor=${collection[collection.length - 1].createdAt.toISOString()}`
        : null,
    };
  }

  async isLiked(sessionId: string, scTrackId: string): Promise<boolean> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    const row = await this.db.query.localLikes.findFirst({
      where: and(
        eq(localLikes.soundcloudUserId, soundcloudUserId),
        eq(localLikes.scTrackId, scTrackId),
      ),
      columns: { id: true },
    });
    return !!row;
  }

  async getLikedTrackIds(sessionId: string, scTrackIds: string[]): Promise<Set<string>> {
    if (scTrackIds.length === 0) return new Set();

    const soundcloudUserId = await this.getScUserId(sessionId);
    const items = await this.db
      .select({ scTrackId: localLikes.scTrackId })
      .from(localLikes)
      .where(
        and(
          eq(localLikes.soundcloudUserId, soundcloudUserId),
          inArray(localLikes.scTrackId, scTrackIds),
        ),
      );
    return new Set(items.map((item) => item.scTrackId));
  }
}
