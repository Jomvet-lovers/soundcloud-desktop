import { Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { AxiosError } from 'axios';
import { Repository } from 'typeorm';
import { AuthService } from '../auth/auth.service.js';
import { OAuthAppsService } from '../oauth-apps/oauth-apps.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { type ActionType, PendingAction } from './entities/pending-action.entity.js';

const MAX_RETRIES = 5;
const SYNC_INTERVAL_MS = 60 * 1000;

@Injectable()
export class PendingActionsService {
  private readonly logger = new Logger(PendingActionsService.name);
  private syncTimer: ReturnType<typeof setInterval> | null = null;

  constructor(
    @InjectRepository(PendingAction)
    private readonly repo: Repository<PendingAction>,
    private readonly sc: SoundcloudService,
    private readonly authService: AuthService,
    private readonly oauthAppsService: OAuthAppsService,
  ) {}

  onModuleInit() {
    this.syncTimer = setInterval(() => {
      this.syncAll().catch((err) => {
        this.logger.error(`Sync failed: ${err.message}`);
      });
    }, SYNC_INTERVAL_MS);
    this.logger.log(`Pending actions sync started (interval: ${SYNC_INTERVAL_MS / 1000}s)`);
  }

  onModuleDestroy() {
    if (this.syncTimer) {
      clearInterval(this.syncTimer);
    }
  }

  /**
   * Проверяет, является ли ошибка баном (403 CloudFront).
   * Если да — экшен можно закинуть в очередь.
   */
  isBanError(error: unknown): boolean {
    if (error instanceof AxiosError && error.response) {
      return this.oauthAppsService.isSoundCloudAppBan(
        error.response.status,
        error.response.data,
      );
    }
    return false;
  }

  /** Добавить действие в очередь */
  async enqueue(
    sessionId: string,
    actionType: ActionType,
    targetUrn: string,
    payload?: Record<string, unknown> | null,
  ): Promise<PendingAction> {
    // Дедупликация: если уже есть pending action с таким же типом и целью — обновить
    const existing = await this.repo.findOne({
      where: { sessionId, actionType, targetUrn, status: 'pending' },
    });

    if (existing) {
      existing.payload = payload ?? existing.payload;
      existing.retryCount = 0;
      existing.error = null;
      return this.repo.save(existing);
    }

    const action = this.repo.create({
      sessionId,
      actionType,
      targetUrn,
      payload: payload ?? null,
      status: 'pending',
    });
    const saved = await this.repo.save(action);
    this.logger.log(`Enqueued: ${actionType} ${targetUrn} for session ${sessionId.slice(0, 8)}...`);
    return saved;
  }

  /** Получить pending actions для сессии */
  async getForSession(sessionId: string): Promise<PendingAction[]> {
    return this.repo.find({
      where: { sessionId, status: 'pending' },
      order: { createdAt: 'ASC' },
    });
  }

  /** Статистика pending actions для сессии */
  async getStats(sessionId: string): Promise<{ pending: number; failed: number }> {
    const [pending, failed] = await Promise.all([
      this.repo.count({ where: { sessionId, status: 'pending' } }),
      this.repo.count({ where: { sessionId, status: 'failed' } }),
    ]);
    return { pending, failed };
  }

  /** Синхронизировать все pending actions */
  async syncAll(): Promise<{ synced: number; failed: number }> {
    // Проверяем, есть ли вообще активные аппки
    try {
      this.oauthAppsService.pickRandomApp();
    } catch {
      // Нет активных аппок — пропускаем sync
      return { synced: 0, failed: 0 };
    }

    const actions = await this.repo.find({
      where: { status: 'pending' },
      order: { createdAt: 'ASC' },
      take: 50,
    });

    if (actions.length === 0) return { synced: 0, failed: 0 };

    this.logger.log(`Syncing ${actions.length} pending actions...`);

    let synced = 0;
    let failed = 0;

    for (const action of actions) {
      try {
        await this.executeAction(action);
        action.status = 'done';
        synced++;
      } catch (err: any) {
        if (this.isBanError(err)) {
          // Все ещё забанены — не увеличиваем retry, просто пропускаем
          this.logger.warn(`Still banned, skipping sync for ${action.actionType} ${action.targetUrn}`);
          break; // Не продолжаем — всё равно забанены
        }

        action.retryCount++;
        action.error = err.message?.slice(0, 500) ?? 'Unknown error';

        if (action.retryCount >= MAX_RETRIES) {
          action.status = 'failed';
          failed++;
          this.logger.warn(`Action failed permanently: ${action.actionType} ${action.targetUrn}`);
        }
      }

      await this.repo.save(action);
    }

    if (synced > 0) {
      this.logger.log(`Sync complete: ${synced} synced, ${failed} failed`);
    }

    return { synced, failed };
  }

  /** Ручной sync для конкретной сессии */
  async syncForSession(sessionId: string): Promise<{ synced: number; failed: number }> {
    const actions = await this.repo.find({
      where: { sessionId, status: 'pending' },
      order: { createdAt: 'ASC' },
    });

    let synced = 0;
    let failed = 0;

    for (const action of actions) {
      try {
        await this.executeAction(action);
        action.status = 'done';
        synced++;
      } catch (err: any) {
        if (this.isBanError(err)) break;

        action.retryCount++;
        action.error = err.message?.slice(0, 500) ?? 'Unknown error';
        if (action.retryCount >= MAX_RETRIES) {
          action.status = 'failed';
          failed++;
        }
      }
      await this.repo.save(action);
    }

    return { synced, failed };
  }

  private async executeAction(action: PendingAction): Promise<void> {
    const token = await this.authService.getValidAccessToken(action.sessionId);

    switch (action.actionType) {
      case 'like':
        await this.sc.apiPost(`/likes/tracks/${action.targetUrn}`, token);
        break;
      case 'unlike':
        await this.sc.apiDelete(`/likes/tracks/${action.targetUrn}`, token);
        break;
      case 'repost':
        await this.sc.apiPost(`/reposts/tracks/${action.targetUrn}`, token);
        break;
      case 'unrepost':
        await this.sc.apiDelete(`/reposts/tracks/${action.targetUrn}`, token);
        break;
      case 'comment':
        await this.sc.apiPost(
          `/tracks/${action.targetUrn}/comments`,
          token,
          action.payload,
        );
        break;
      case 'like_playlist':
        await this.sc.apiPost(`/likes/playlists/${action.targetUrn}`, token);
        break;
      case 'unlike_playlist':
        await this.sc.apiDelete(`/likes/playlists/${action.targetUrn}`, token);
        break;
      case 'repost_playlist':
        await this.sc.apiPost(`/reposts/playlists/${action.targetUrn}`, token);
        break;
      case 'unrepost_playlist':
        await this.sc.apiDelete(`/reposts/playlists/${action.targetUrn}`, token);
        break;
      case 'playlist_create':
        await this.sc.apiPost('/playlists', token, action.payload);
        break;
      case 'playlist_update':
        await this.sc.apiPut(`/playlists/${action.targetUrn}`, token, action.payload);
        break;
      case 'playlist_delete':
        await this.sc.apiDelete(`/playlists/${action.targetUrn}`, token);
        break;
      default:
        this.logger.warn(`Unknown action type: ${action.actionType}`);
    }
  }
}
