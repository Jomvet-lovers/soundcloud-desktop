import { Inject, Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { QdrantClient } from '@qdrant/js-client-rest';

/**
 * Кеш и периодический refresh центроидов tracks_mert и tracks_clap.
 *
 * MuQ-large-msd-iter и MuQ-MuLan-large дают эмбеддинги с очень высокой baseline
 * cosine similarity между любыми двумя треками (~0.78 / 0.80). Из-за этого user_taste
 * и popular-tracks стягиваются к центроиду коллекции, и cos сильно теряет различительную
 * силу. Решение — whitening: вычитать центроид перед сравнением, тогда cos считается
 * по «отклонениям от среднего трека», и результаты становятся осмысленными.
 *
 * lyrics НЕ whiten-им: там пространство хорошо структурировано (MiniLM/multilingual),
 * baseline ниже и whitening бы не помог, а только бы шумил.
 */
const SAMPLE_SIZE = 2000;
const REFRESH_MS = 60 * 60 * 1000;

interface CentroidEntry {
  vector: number[] | null;
  updatedAt: number;
  pointsCount: number;
}

@Injectable()
export class CentroidService implements OnModuleInit {
  private readonly logger = new Logger(CentroidService.name);
  private readonly cache = new Map<string, CentroidEntry>();
  private timer: NodeJS.Timeout | null = null;

  constructor(
    @Inject('QDRANT_CLIENT')
    private readonly qdrant: QdrantClient,
  ) {}

  async onModuleInit() {
    await this.refreshAll();
    this.timer = setInterval(() => {
      this.refreshAll().catch((e) =>
        this.logger.warn(`centroid refresh failed: ${(e as Error).message}`),
      );
    }, REFRESH_MS);
    this.timer.unref?.();
  }

  /** Вектор-центроид (нормализованный) или null если коллекция пуста / недоступна. */
  get(collection: 'tracks_mert' | 'tracks_clap'): number[] | null {
    return this.cache.get(collection)?.vector ?? null;
  }

  /**
   * Whitened cosine: cos(norm(a-c), norm(b-c)).
   * Если центроида нет — fallback на обычный cos.
   */
  whitenedCosine(a: number[], b: number[], centroid: number[] | null): number {
    if (!centroid) return cosine(a, b);
    const aw = subtract(a, centroid);
    const bw = subtract(b, centroid);
    return cosine(aw, bw);
  }

  private async refreshAll() {
    await Promise.all([this.refresh('tracks_mert'), this.refresh('tracks_clap')]);
  }

  private async refresh(collection: 'tracks_mert' | 'tracks_clap') {
    try {
      let next: string | number | undefined | null;
      const acc: number[] = [];
      let count = 0;
      do {
        const res = (await this.qdrant.scroll(collection, {
          limit: Math.min(256, SAMPLE_SIZE - count),
          with_vector: true,
          with_payload: false,
          offset: next ?? undefined,
        })) as { points: Array<{ vector?: number[] | null }>; next_page_offset?: string | number };
        for (const p of res.points) {
          if (!Array.isArray(p.vector) || p.vector.length === 0) continue;
          if (acc.length === 0) acc.length = p.vector.length;
          for (let i = 0; i < p.vector.length; i++) acc[i] = (acc[i] ?? 0) + p.vector[i];
          count++;
        }
        next = res.next_page_offset ?? null;
        if (count >= SAMPLE_SIZE) break;
      } while (next);

      if (count === 0) {
        this.cache.set(collection, { vector: null, updatedAt: Date.now(), pointsCount: 0 });
        this.logger.warn(`centroid ${collection}: empty collection`);
        return;
      }

      const mean = acc.map((v) => v / count);
      const n = Math.sqrt(mean.reduce((s, v) => s + v * v, 0));
      const normalized = n > 0 ? mean.map((v) => v / n) : mean;
      this.cache.set(collection, {
        vector: normalized,
        updatedAt: Date.now(),
        pointsCount: count,
      });
      this.logger.log(
        `centroid ${collection} refreshed: sampled=${count} dim=${normalized.length} norm=${n.toFixed(3)}`,
      );
    } catch (e) {
      this.logger.warn(`centroid ${collection} refresh failed: ${(e as Error).message}`);
    }
  }
}

function subtract(a: number[], b: number[]): number[] {
  const n = Math.min(a.length, b.length);
  const out = new Array<number>(n);
  for (let i = 0; i < n; i++) out[i] = a[i] - b[i];
  return out;
}

function cosine(a: number[], b: number[]): number {
  const n = Math.min(a.length, b.length);
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < n; i++) {
    dot += a[i] * b[i];
    na += a[i] * a[i];
    nb += b[i] * b[i];
  }
  const denom = Math.sqrt(na) * Math.sqrt(nb);
  return denom > 0 ? dot / denom : 0;
}
