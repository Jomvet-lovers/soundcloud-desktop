import { forwardRef, Inject, Injectable, UnauthorizedException } from '@nestjs/common';
import { and, desc, eq, inArray, lt } from 'drizzle-orm';
import { AuthService } from '../auth/auth.service.js';
import { normalizeScTrackId } from '../common/sc-ids.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { dislikedTracks } from '../db/schema.js';
import { EventsService } from '../events/events.service.js';

@Injectable()
export class DislikesService {
  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly authService: AuthService,
    @Inject(forwardRef(() => EventsService))
    private readonly events: EventsService,
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
    trackData?: Record<string, unknown>,
  ): Promise<{ status: string }> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return { status: 'invalid' };
    const scUserId = await this.getScUserId(sessionId);

    const inserted = await this.db
      .insert(dislikedTracks)
      .values({ scUserId, scTrackId: id, trackData: trackData ?? null })
      .onConflictDoNothing({ target: [dislikedTracks.scUserId, dislikedTracks.scTrackId] })
      .returning({ id: dislikedTracks.id });

    if (inserted.length > 0) {
      await this.events.record(scUserId, id, 'dislike');
    }
    return { status: 'ok' };
  }

  async remove(sessionId: string, scTrackId: string): Promise<{ status: string }> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return { status: 'invalid' };
    const scUserId = await this.getScUserId(sessionId);
    await this.db
      .delete(dislikedTracks)
      .where(and(eq(dislikedTracks.scUserId, scUserId), eq(dislikedTracks.scTrackId, id)));
    return { status: 'removed' };
  }

  async isDisliked(sessionId: string, scTrackId: string): Promise<boolean> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return false;
    const scUserId = await this.getScUserId(sessionId);
    return this.isDislikedByUserId(scUserId, id);
  }

  async isDislikedByUserId(scUserId: string, scTrackId: string): Promise<boolean> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return false;
    const row = await this.db.query.dislikedTracks.findFirst({
      where: and(eq(dislikedTracks.scUserId, scUserId), eq(dislikedTracks.scTrackId, id)),
      columns: { id: true },
    });
    return !!row;
  }

  async getDislikedTrackIds(sessionId: string, scTrackIds: string[]): Promise<Set<string>> {
    if (scTrackIds.length === 0) return new Set();
    const normalized = scTrackIds
      .map((id) => normalizeScTrackId(id))
      .filter((id): id is string => !!id);
    if (normalized.length === 0) return new Set();
    const scUserId = await this.getScUserId(sessionId);
    const items = await this.db
      .select({ scTrackId: dislikedTracks.scTrackId })
      .from(dislikedTracks)
      .where(
        and(eq(dislikedTracks.scUserId, scUserId), inArray(dislikedTracks.scTrackId, normalized)),
      );
    return new Set(items.map((item) => item.scTrackId));
  }

  async listIdsByUserId(scUserId: string, limit = 200): Promise<string[]> {
    const items = await this.db
      .select({ scTrackId: dislikedTracks.scTrackId })
      .from(dislikedTracks)
      .where(eq(dislikedTracks.scUserId, scUserId))
      .orderBy(desc(dislikedTracks.createdAt))
      .limit(limit);
    return items.map((item) => item.scTrackId);
  }

  async listIdsBySession(sessionId: string, limit = 1000): Promise<string[]> {
    const scUserId = await this.getScUserId(sessionId);
    return this.listIdsByUserId(scUserId, limit);
  }

  async findAll(
    sessionId: string,
    limit: number,
    cursor?: string,
  ): Promise<{ collection: Record<string, unknown>[]; next_href: string | null }> {
    const scUserId = await this.getScUserId(sessionId);
    const conds = [eq(dislikedTracks.scUserId, scUserId)];
    if (cursor) conds.push(lt(dislikedTracks.createdAt, new Date(cursor)));

    const items = await this.db
      .select()
      .from(dislikedTracks)
      .where(and(...conds))
      .orderBy(desc(dislikedTracks.createdAt))
      .limit(limit + 1);

    const hasMore = items.length > limit;
    const collection = items.slice(0, limit);
    return {
      collection: collection
        .map((item) => item.trackData)
        .filter((d): d is Record<string, unknown> => d !== null),
      next_href: hasMore
        ? `?limit=${limit}&cursor=${collection[collection.length - 1].createdAt.toISOString()}`
        : null,
    };
  }
}
