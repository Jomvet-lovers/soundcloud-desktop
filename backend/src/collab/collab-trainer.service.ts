import { Inject, Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';
import { asc, gte } from 'drizzle-orm';
import { NatsService } from '../bus/nats.service.js';
import { SUBJECTS } from '../bus/subjects.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { userEvents } from '../db/schema.js';
import { CollabVectorService } from './collab-vector.service.js';

const SESSION_GAP_MS = 30 * 60 * 1000;
const MIN_SESSION_LEN = 2;
const MAX_SESSION_LEN = 200;
const HISTORY_WINDOW_DAYS = 90;
const DEFAULT_MIN_SESSIONS = 20;

const SESSION_EVENTS = new Set(['like', 'local_like', 'playlist_add', 'full_play', 'skip']);

@Injectable()
export class CollabTrainerService implements OnModuleInit {
  private readonly logger = new Logger(CollabTrainerService.name);
  private inProgress = false;
  private eventCounter = 0;
  private lastTrainAt = 0;

  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly nats: NatsService,
    private readonly collab: CollabVectorService,
  ) {}

  async onModuleInit(): Promise<void> {
    setTimeout(() => {
      void this.bootstrapIfNeeded();
    }, 30_000);
  }

  private async bootstrapIfNeeded(): Promise<void> {
    try {
      const dim = await this.collab.getCollabDim();
      if (dim !== null) {
        this.logger.log(`[collab.bootstrap] tracks_collab exists (dim=${dim}), skip initial train`);
        return;
      }
      this.logger.log('[collab.bootstrap] tracks_collab missing, triggering initial train');
      const res = await this.trainNow();
      this.logger.log(
        `[collab.bootstrap] result: enqueued=${res.enqueued} sessions=${res.sessions}` +
          (res.reason ? ` reason=${res.reason}` : ''),
      );
    } catch (e) {
      this.logger.warn(`[collab.bootstrap] failed: ${(e as Error).message}`);
    }
  }

  @Cron(CronExpression.EVERY_6_HOURS)
  async scheduledTrain(): Promise<void> {
    if (process.env.COLLAB_AUTO_TRAIN === 'false') return;
    await this.trainNow().catch((e) =>
      this.logger.warn(`scheduled train failed: ${(e as Error).message}`),
    );
  }

  noteEvent(): void {
    this.eventCounter++;
    const threshold = Number.parseInt(process.env.COLLAB_TRIGGER_EVENTS ?? '100', 10);
    const cooldownMs = Number.parseInt(process.env.COLLAB_TRIGGER_COOLDOWN_MS ?? '600000', 10);
    if (this.eventCounter < threshold) return;
    if (Date.now() - this.lastTrainAt < cooldownMs) return;
    if (this.inProgress) return;
    this.eventCounter = 0;
    this.logger.log(`[collab.auto] threshold reached, triggering train`);
    void this.trainNow().catch((e) =>
      this.logger.warn(`auto train failed: ${(e as Error).message}`),
    );
  }

  async trainNow(opts: { dim?: number; minCount?: number } = {}): Promise<{
    enqueued: boolean;
    sessions: number;
    reason?: string;
  }> {
    if (this.inProgress) {
      return { enqueued: false, sessions: 0, reason: 'already_in_progress' };
    }
    this.inProgress = true;
    try {
      const sessions = await this.buildSessions();
      const minSessions = Number.parseInt(
        process.env.COLLAB_MIN_SESSIONS ?? String(DEFAULT_MIN_SESSIONS),
        10,
      );
      if (sessions.length < minSessions) {
        this.logger.warn(
          `[collab.train] too few sessions (${sessions.length} < ${minSessions}), skip. ` +
            `Lower COLLAB_MIN_SESSIONS to train on less data.`,
        );
        return { enqueued: false, sessions: sessions.length, reason: 'too_few_sessions' };
      }

      const dim = opts.dim ?? Number.parseInt(process.env.COLLAB_DIM ?? '128', 10);
      const minCount = opts.minCount ?? Number.parseInt(process.env.COLLAB_MIN_COUNT ?? '3', 10);

      this.logger.log(
        `[collab.train] enqueuing ${sessions.length} sessions, dim=${dim}, min_count=${minCount}`,
      );
      await this.nats.publish(SUBJECTS.trainCollab, {
        sessions,
        dim,
        min_count: minCount,
        window: 5,
        epochs: 5,
        negative: 10,
      });
      this.collab.invalidateAll();
      this.lastTrainAt = Date.now();
      return { enqueued: true, sessions: sessions.length };
    } finally {
      this.inProgress = false;
    }
  }

  private async buildSessions(): Promise<number[][]> {
    const since = new Date(Date.now() - HISTORY_WINDOW_DAYS * 24 * 60 * 60 * 1000);
    const rows = await this.db
      .select({
        scUserId: userEvents.scUserId,
        scTrackId: userEvents.scTrackId,
        createdAt: userEvents.createdAt,
        eventType: userEvents.eventType,
      })
      .from(userEvents)
      .where(gte(userEvents.createdAt, since))
      .orderBy(asc(userEvents.scUserId), asc(userEvents.createdAt));

    const sessions: number[][] = [];
    let currentUser: string | null = null;
    let currentTime = 0;
    let currentSession: number[] = [];
    let currentSeen = new Set<number>();

    const flush = () => {
      if (currentSession.length >= MIN_SESSION_LEN) {
        sessions.push(currentSession.slice(0, MAX_SESSION_LEN));
      }
      currentSession = [];
      currentSeen = new Set();
    };

    for (const r of rows) {
      if (!SESSION_EVENTS.has(r.eventType)) continue;
      const tid = Number(r.scTrackId);
      if (!Number.isFinite(tid)) continue;
      const ts = new Date(r.createdAt).getTime();

      if (r.scUserId !== currentUser) {
        flush();
        currentUser = r.scUserId;
        currentTime = ts;
      } else if (ts - currentTime > SESSION_GAP_MS) {
        flush();
        currentTime = ts;
      } else {
        currentTime = ts;
      }

      if (!currentSeen.has(tid)) {
        currentSession.push(tid);
        currentSeen.add(tid);
      }
    }
    flush();

    this.logger.log(
      `[collab.train] built ${sessions.length} sessions from ${rows.length} events ` +
        `(${HISTORY_WINDOW_DAYS}d window)`,
    );
    return sessions;
  }
}
