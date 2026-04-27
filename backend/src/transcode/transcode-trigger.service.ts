import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { firstValueFrom } from 'rxjs';
import { NatsService } from '../bus/nats.service.js';
import { SUBJECTS } from '../bus/subjects.js';

/**
 * Тонкий fire-and-forget триггер на streaming /internal/transcode-upload.
 * Используется и индексингом (новый трек), и lyrics-reap'ом (повторный whisper-проход).
 *
 * Поведение:
 *  - Streaming отвечает быстро (HEAD storage; cached → мгновенно, иначе уходит в свой
 *    fetch+upload и ответит позже).
 *  - На cached:true сразу publish'им `storage.track_uploaded` локально — subscribers
 *    (indexing + lyrics handleUploaded) запускаются без ожидания реального upload.
 *  - На cached:false ничего не делаем — настоящий event прилетит от storage.
 */
@Injectable()
export class TranscodeTriggerService {
  private readonly logger = new Logger(TranscodeTriggerService.name);

  constructor(
    private readonly http: HttpService,
    private readonly config: ConfigService,
    private readonly nats: NatsService,
  ) {}

  trigger(scTrackId: string): void {
    const streamingUrl = this.config.getOrThrow<string>('streaming.serviceUrl');
    const token = this.config.getOrThrow<string>('internal.token');

    firstValueFrom(
      this.http.post<{ url: string; size_bytes: number; cached: boolean }>(
        `${streamingUrl}/internal/transcode-upload/${encodeURIComponent(scTrackId)}`,
        {},
        {
          timeout: 30 * 1000,
          headers: { Authorization: `Bearer ${token}` },
        },
      ),
    )
      .then(async (resp) => {
        const { url, cached } = resp.data;
        if (cached) {
          await this.nats.publish(SUBJECTS.storageTrackUploaded, {
            sc_track_id: scTrackId,
            storage_url: url,
          });
          this.logger.debug(`[trigger] ${scTrackId} cached → fanned out`);
        }
      })
      .catch((e) => {
        this.logger.debug(`[trigger] ${scTrackId} failed: ${(e as Error).message}`);
      });
  }
}
