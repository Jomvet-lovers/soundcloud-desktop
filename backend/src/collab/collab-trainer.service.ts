import { Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { Cron, CronExpression } from '@nestjs/schedule';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { NatsService } from '../bus/nats.service.js';
import { SUBJECTS } from '../bus/subjects.js';
import { UserEvent } from '../events/entities/user-event.entity.js';
import { CollabVectorService } from './collab-vector.service.js';

/**
 * Сборка сессий прослушивания и отправка в воркер для тренировки item2vec.
 *
 * Триггеры тренировки:
 *   1. Cron каждые COLLAB_CRON_HOURS часов (по умолчанию 6h).
 *   2. Auto-debounce: после N positive-событий с прошлой тренировки
 *      (COLLAB_TRIGGER_EVENTS, default 100) — фоном запускается trainNow.
 *      Это даёт быстрый отклик на «активный день» юзеров без ручных триггеров.
 *   3. Bootstrap при старте: если tracks_collab коллекции ещё нет, а сессий
 *      хватает — тренируем сразу (с задержкой 30с чтобы NATS успел подняться).
 *
 * Сессия = последовательность positive/skip-событий одного юзера, разделённых
 * паузой ≤30 минут. Для item2vec лучше брать ВСЕ события (не только positive),
 * т.к. сама последовательность прослушивания «треки слушают вместе» — это сигнал
 * сходства независимо от того like юзер поставил или просто играл.
 */
const SESSION_GAP_MS = 30 * 60 * 1000;
const MIN_SESSION_LEN = 2;
const MAX_SESSION_LEN = 200;
const HISTORY_WINDOW_DAYS = 90;
const MIN_TOTAL_SESSIONS = 100;

const SESSION_EVENTS = new Set(['like', 'local_like', 'playlist_add', 'full_play', 'skip']);

@Injectable()
export class CollabTrainerService implements OnModuleInit {
  private readonly logger = new Logger(CollabTrainerService.name);
  private inProgress = false;
  private eventCounter = 0;
  private lastTrainAt = 0;

  constructor(
    @InjectRepository(UserEvent)
    private readonly events: Repository<UserEvent>,
    private readonly nats: NatsService,
    private readonly collab: CollabVectorService,
  ) {}

  async onModuleInit(): Promise<void> {
    // Bootstrap: если коллекция collab ещё не существует — попробовать обучить
    // через 30с после старта (даём NATS/воркеру время подняться).
    setTimeout(() => {
      void this.bootstrapIfNeeded();
    }, 30_000);
  }

  private async bootstrapIfNeeded(): Promise<void> {
    try {
      const dim = await this.collab.getCollabDim();
      if (dim !== null) return;
      this.logger.log('[collab.bootstrap] tracks_collab missing, triggering initial train');
      await this.trainNow();
    } catch (e) {
      this.logger.warn(`[collab.bootstrap] failed: ${(e as Error).message}`);
    }
  }

  /** По умолчанию каждые 6 часов. */
  @Cron(CronExpression.EVERY_6_HOURS)
  async scheduledTrain(): Promise<void> {
    if (process.env.COLLAB_AUTO_TRAIN === 'false') return;
    await this.trainNow().catch((e) =>
      this.logger.warn(`scheduled train failed: ${(e as Error).message}`),
    );
  }

  /**
   * Уведомление о новом session-events событии. Вызывается из EventsService
   * на каждое positive/skip/full_play. Когда накопилось ≥ COLLAB_TRIGGER_EVENTS
   * с прошлой тренировки — фоном запускаем trainNow (debounced cooldown 10 мин).
   */
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

  /** Ручной запуск. Идемпотентен (не запускает второй параллельно). */
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
      if (sessions.length < MIN_TOTAL_SESSIONS) {
        this.logger.warn(
          `[collab.train] too few sessions (${sessions.length} < ${MIN_TOTAL_SESSIONS}), skip`,
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
      // Кеш юзеров инвалидируем после успешного train (его сообщит done.train_collab).
      // Сейчас просто помечаем чтобы dim перепроверился.
      this.collab.invalidateAll();
      this.lastTrainAt = Date.now();
      return { enqueued: true, sessions: sessions.length };
    } finally {
      this.inProgress = false;
    }
  }

  /**
   * Собирает сессии из user_events: события одного юзера в хронологическом
   * порядке, разделённые гэпом > SESSION_GAP_MS, считаются разными сессиями.
   * Дубликаты track_id внутри сессии убираем (повторное прослушивание не несёт
   * дополнительной информации для co-listen сигнала).
   */
  private async buildSessions(): Promise<number[][]> {
    const since = new Date(Date.now() - HISTORY_WINDOW_DAYS * 24 * 60 * 60 * 1000);
    const rows: Array<{ scUserId: string; scTrackId: string; createdAt: Date; eventType: string }> =
      await this.events
        .createQueryBuilder('e')
        .select([
          'e.scUserId AS "scUserId"',
          'e.scTrackId AS "scTrackId"',
          'e.createdAt AS "createdAt"',
          'e.eventType AS "eventType"',
        ])
        .where('e.createdAt >= :since', { since })
        .orderBy('e.scUserId', 'ASC')
        .addOrderBy('e.createdAt', 'ASC')
        .getRawMany();

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
      `[collab.train] built ${sessions.length} sessions from ${rows.length} events (${HISTORY_WINDOW_DAYS}d window)`,
    );
    return sessions;
  }
}
