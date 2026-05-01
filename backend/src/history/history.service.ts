import { Inject, Injectable, UnauthorizedException } from '@nestjs/common';
import { and, count, desc, eq, gt } from 'drizzle-orm';
import { AuthService } from '../auth/auth.service.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { type ListeningHistory, listeningHistory } from '../db/schema.js';

@Injectable()
export class HistoryService {
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

  async record(
    sessionId: string,
    data: {
      scTrackId: string;
      title: string;
      artistName: string;
      artistUrn?: string;
      artworkUrl?: string;
      duration: number;
    },
  ): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);

    const recent = await this.db.query.listeningHistory.findFirst({
      where: and(
        eq(listeningHistory.soundcloudUserId, soundcloudUserId),
        eq(listeningHistory.scTrackId, data.scTrackId),
        gt(listeningHistory.playedAt, new Date(Date.now() - 60_000)),
      ),
      orderBy: desc(listeningHistory.playedAt),
    });
    if (recent) return;

    await this.db.insert(listeningHistory).values({ soundcloudUserId, ...data });
  }

  async findAll(
    sessionId: string,
    limit: number,
    offset: number,
  ): Promise<{ collection: ListeningHistory[]; total: number }> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    const where = eq(listeningHistory.soundcloudUserId, soundcloudUserId);

    const [collection, totalRows] = await Promise.all([
      this.db
        .select()
        .from(listeningHistory)
        .where(where)
        .orderBy(desc(listeningHistory.playedAt))
        .limit(limit)
        .offset(offset),
      this.db.select({ n: count() }).from(listeningHistory).where(where),
    ]);
    return { collection, total: totalRows[0]?.n ?? 0 };
  }

  async clear(sessionId: string): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    await this.db
      .delete(listeningHistory)
      .where(eq(listeningHistory.soundcloudUserId, soundcloudUserId));
  }
}
