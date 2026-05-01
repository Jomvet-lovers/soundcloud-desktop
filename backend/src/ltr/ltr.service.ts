import { Injectable, Logger } from '@nestjs/common';
import { NatsService } from '../bus/nats.service.js';
import { SUBJECTS } from '../bus/subjects.js';

/**
 * Длина feature-вектора. Должна совпадать с N_FEATURES в worker/src/handlers/ltr.py.
 *
 * Индексы фичей:
 *   0: collab_cos              — поведенческий сигнал (item2vec)
 *   1: mert_whitened_cos       — аудио (MuQ)
 *   2: clap_whitened_cos       — аудио (MuQ-MuLan)
 *   3: lyrics_cos              — текстовый
 *   4: log1p(playback_count)   — популярность
 *   5: language_match          — 1.0 если язык трека ∈ preferences юзера, иначе 0.0
 */
export const LTR_FEATURE_COUNT = 6;

const RPC_TIMEOUT_MS = 1500;

@Injectable()
export class LtrService {
  private readonly logger = new Logger(LtrService.name);

  constructor(private readonly nats: NatsService) {}

  get enabled(): boolean {
    return process.env.LTR_RERANK_ENABLED !== 'false';
  }

  /**
   * Возвращает массив скоров (по одному на кандидата) от LightGBM-ranker воркера.
   * Если воркер недоступен / модель не обучена / RPC timeout — возвращает null,
   * вызывающий код использует fallback (линейная сумма cosines).
   */
  async score(features: number[][]): Promise<number[] | null> {
    if (!this.enabled || !features.length) return null;
    try {
      const r = await this.nats.request<{ scores: number[]; fallback?: boolean }>(
        SUBJECTS.aiLtrScore,
        { features },
        RPC_TIMEOUT_MS,
      );
      if (!r || !Array.isArray(r.scores)) return null;
      if (r.scores.length !== features.length) {
        this.logger.warn(
          `ltr.score length mismatch: got=${r.scores.length} want=${features.length}`,
        );
        return null;
      }
      if (r.fallback) {
        this.logger.debug('ltr.score using fallback (no trained model on worker)');
      }
      return r.scores;
    } catch (e) {
      this.logger.debug(`ltr.score failed: ${(e as Error).message}`);
      return null;
    }
  }

  /** Триггер обучения LTR-модели — backend сам собирает примеры и публикует. */
  async publishTraining(
    examples: Array<{ group: number; label: number; features: number[] }>,
  ): Promise<void> {
    if (!examples.length) return;
    await this.nats.publish(SUBJECTS.trainLtr, { examples });
  }
}
