import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { firstValueFrom } from 'rxjs';
import { In, Repository } from 'typeorm';
import { IndexedTrack } from '../indexing/entities/indexed-track.entity.js';

const MISS_TTL_MS = 24 * 60 * 60 * 1000;
const HEAD_CONCURRENCY = 16;
const HEAD_TIMEOUT_MS = 3000;

function pLimit(max: number) {
  let active = 0;
  const queue: Array<() => void> = [];
  const next = () => {
    if (active >= max) return;
    const job = queue.shift();
    if (job) {
      active++;
      job();
    }
  };
  return <T>(fn: () => Promise<T>): Promise<T> =>
    new Promise<T>((resolve, reject) => {
      queue.push(() => {
        fn()
          .then(resolve, reject)
          .finally(() => {
            active--;
            next();
          });
      });
      next();
    });
}

/**
 * Lazy S3 truth для рекомендаций.
 *
 * Positive cache: indexed_tracks.s3_verified_at — обновляется на NATS event
 * `storage.track_uploaded` или после успешного HEAD.
 * Negative cache: indexed_tracks.s3_missing_at с TTL = 24h.
 *
 * Фильтр в рекомендациях вызывает findMissing() уже на финальной выборке
 * (top-N после artistCap), не на всём пуле кандидатов из Qdrant.
 */
@Injectable()
export class S3VerifierService {
  private readonly logger = new Logger(S3VerifierService.name);
  private readonly storageUrl: string;
  private readonly limit = pLimit(HEAD_CONCURRENCY);

  constructor(
    private readonly http: HttpService,
    config: ConfigService,
    @InjectRepository(IndexedTrack)
    private readonly repo: Repository<IndexedTrack>,
  ) {
    this.storageUrl = (config.get<string>('storage.url') ?? '').replace(/\/+$/, '');
  }

  /** Возвращает Set scTrackId, у которых файла в S3 точно нет. */
  async findMissing(scTrackIds: string[]): Promise<Set<string>> {
    const missing = new Set<string>();
    if (!scTrackIds.length || !this.storageUrl) return missing;

    const ttlCutoff = new Date(Date.now() - MISS_TTL_MS);

    const rows = await this.repo.find({
      where: { scTrackId: In(scTrackIds) },
      select: ['scTrackId', 's3VerifiedAt', 's3MissingAt'],
    });
    const byId = new Map(rows.map((r) => [r.scTrackId, r]));

    const toCheck: string[] = [];
    for (const id of scTrackIds) {
      const r = byId.get(id);
      if (r?.s3VerifiedAt && (!r.s3MissingAt || r.s3MissingAt <= r.s3VerifiedAt)) {
        continue;
      }
      if (r?.s3MissingAt && r.s3MissingAt > ttlCutoff) {
        missing.add(id);
        continue;
      }
      toCheck.push(id);
    }

    if (!toCheck.length) return missing;

    const checked = await Promise.all(toCheck.map((id) => this.limit(() => this.probe(id))));

    const okIds: string[] = [];
    const missIds: string[] = [];
    checked.forEach((found, i) => {
      const id = toCheck[i];
      if (found) {
        okIds.push(id);
      } else {
        missIds.push(id);
        missing.add(id);
      }
    });

    if (okIds.length) {
      await this.repo
        .createQueryBuilder()
        .update(IndexedTrack)
        .set({ s3VerifiedAt: () => 'now()', s3MissingAt: null })
        .where({ scTrackId: In(okIds) })
        .execute();
    }
    if (missIds.length) {
      await this.repo
        .createQueryBuilder()
        .update(IndexedTrack)
        .set({ s3MissingAt: () => 'now()' })
        .where({ scTrackId: In(missIds) })
        .execute();
      this.logger.debug(`S3 miss x${missIds.length} (ok=${okIds.length})`);
    }

    return missing;
  }

  private async probe(scTrackId: string): Promise<boolean> {
    const filename = `soundcloud_tracks_${scTrackId}.ogg`;
    for (const quality of ['hq', 'sq'] as const) {
      const url = `${this.storageUrl}/${quality}/${filename}`;
      try {
        const resp = await firstValueFrom(
          this.http.head(url, { timeout: HEAD_TIMEOUT_MS, validateStatus: () => true }),
        );
        if (resp.status >= 200 && resp.status < 300) return true;
        if (resp.status !== 404 && resp.status !== 410) {
          this.logger.debug(`HEAD ${url} → ${resp.status}`);
          return false;
        }
      } catch (e) {
        this.logger.debug(`HEAD ${url} failed: ${(e as Error).message}`);
        return false;
      }
    }
    return false;
  }
}
