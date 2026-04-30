import { createHash } from 'node:crypto';
import { Inject, Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { QdrantClient } from '@qdrant/js-client-rest';
import { In, IsNull, Not, Repository } from 'typeorm';
import { IndexedTrack } from '../indexing/entities/indexed-track.entity.js';
import { WorkerClient } from '../lyrics/worker.client.js';
import { S3VerifierService } from './s3-verifier.service.js';

interface QdrantFilter {
  must_not?: Array<{ key: string; match: { value: string } }>;
  should?: Array<{ key: string; match: { value: string } }>;
  must?: Array<{ key: string; match: { value: string } | { any: string[] } }>;
}

export interface RecommendResult {
  id: string | number;
  score?: number;
  payload?: Record<string, unknown>;
  artist?: string | null;
  genre?: string | null;
  playbackCount?: number;
}

const RRF_K = 60;
// λ = 1 - DIVERSE_DIVERSITY. 0.7 → λ=0.3: MMR ещё учитывает релевантность,
// но разбавляет результат разнообразием. 1.0 (полный спред) уводит в кластер-шум.
const DIVERSE_DIVERSITY = 0.7;

interface Branch {
  name: string;
  trackCollection: string;
  tasteCollection: string;
  weight: number;
}

interface PoolEntry {
  id: string | number;
  mertScore: number;
  payload?: Record<string, unknown>;
}

export type WaveMode = 'similar' | 'diverse';

interface SimilarInput {
  anchorTrackId: number;
  exclude: string[];
  limit: number;
  languages?: string[];
  diversity: number;
}

function parseIdOrNull(raw: string): number | null {
  const s = String(raw).trim();
  const last = s.includes(':') ? (s.split(':').pop() ?? '') : s;
  if (!/^\d+$/.test(last)) return null;
  const n = Number.parseInt(last, 10);
  return Number.isNaN(n) ? null : n;
}

@Injectable()
export class RecommendationsService {
  private readonly logger = new Logger(RecommendationsService.name);

  constructor(
    @Inject('QDRANT_CLIENT')
    private readonly qdrant: QdrantClient,
    @InjectRepository(IndexedTrack)
    private readonly tracksRepo: Repository<IndexedTrack>,
    private readonly worker: WorkerClient,
    private readonly s3: S3VerifierService,
  ) {}

  private get weightMert(): number {
    return Number.parseFloat(process.env.SOUNDWAVE_AUDIO_WEIGHT ?? '0.30');
  }

  private get weightClap(): number {
    return Number.parseFloat(process.env.SOUNDWAVE_CLAP_WEIGHT ?? '0.20');
  }

  private get weightLyrics(): number {
    return Number.parseFloat(process.env.SOUNDWAVE_LYRICS_WEIGHT ?? '0.50');
  }

  private get popularityBoost(): number {
    return Number.parseFloat(process.env.SOUNDWAVE_POPULARITY_BOOST ?? '0');
  }

  private get artistCapN(): number {
    return Number.parseInt(process.env.SOUNDWAVE_ARTIST_CAP ?? '2', 10);
  }

  private get scoreThreshold(): number {
    return Number.parseFloat(process.env.SOUNDWAVE_SCORE_THRESHOLD ?? '0.3');
  }

  private get branches(): Branch[] {
    return [
      {
        name: 'mert',
        trackCollection: 'tracks_mert',
        tasteCollection: 'user_taste_mert',
        weight: this.weightMert,
      },
      {
        name: 'clap',
        trackCollection: 'tracks_clap',
        tasteCollection: 'user_taste_clap',
        weight: this.weightClap,
      },
      {
        name: 'lyrics',
        trackCollection: 'tracks_lyrics',
        tasteCollection: 'user_taste_lyrics',
        weight: this.weightLyrics,
      },
    ];
  }

  /**
   * Taste-aware SoundWave feed. Оба режима используют 3-base scoring:
   *   - кандидаты из tracks_mert (recommend по user_taste_mert + опц. anchor),
   *   - score = w_m·cos(track.mert,user_taste.mert) + w_c·cos(track.clap,user_taste.clap)
   *           + w_l·cos(track.lyrics,user_taste.lyrics); 0 если вектора нет.
   * Различия:
   *   - 'similar': меньший пул, threshold=default, без MMR, sort by score.
   *   - 'diverse': больший пул, threshold ниже, после сортировки — MMR (λ=0.3).
   */
  async wave(
    scUserId: string,
    scTrackId: string | null,
    positive: string[],
    negative: string[],
    exclude: string[],
    limit = 20,
    languages?: string[],
    mode: WaveMode = 'similar',
    reqId = '-',
  ): Promise<RecommendResult[]> {
    const anchorTrackId = scTrackId ? parseIdOrNull(scTrackId) : null;
    const positiveIds = positive.map(parseIdOrNull).filter((n): n is number => n !== null);
    const negativeIds = negative.map(parseIdOrNull).filter((n): n is number => n !== null);

    this.logger.log(
      `[${reqId}] wave() start  mode=${mode} scTrackIdRaw=${scTrackId ?? 'null'} ` +
        `anchorTrackId=${anchorTrackId ?? 'null'} positiveIds=${positiveIds.length} ` +
        `negativeIds=${negativeIds.length} exclude=${exclude.length} ` +
        `limit=${limit} weights={mert:${this.weightMert},clap:${this.weightClap},lyrics:${this.weightLyrics}}`,
    );

    const result =
      mode === 'diverse'
        ? await this.waveDiverse(
            scUserId,
            anchorTrackId,
            positiveIds,
            negativeIds,
            exclude,
            limit,
            languages,
            reqId,
          )
        : await this.waveSimilar(
            scUserId,
            anchorTrackId,
            positiveIds,
            negativeIds,
            exclude,
            limit,
            languages,
            reqId,
          );

    if (result.length >= 5) {
      this.logger.log(
        `[${reqId}] wave() ok  mode=${mode} returned=${result.length} top5=[${result
          .slice(0, 5)
          .map((r) => `${r.id}:${(r.score ?? 0).toFixed(3)}`)
          .join(',')}]`,
      );
      return result;
    }
    this.logger.warn(
      `[${reqId}] wave() FALLBACK  mode=${mode} only=${result.length} tracks — using getFallbackTracks()`,
    );
    return this.getFallbackTracks(exclude, limit, languages);
  }

  /** Публичный feed без anchor — обёртка поверх wave(). */
  recommend(
    scUserId: string,
    positive: string[],
    negative: string[],
    exclude: string[],
    limit = 20,
    languages?: string[],
    mode: WaveMode = 'similar',
    reqId = '-',
  ) {
    return this.wave(scUserId, null, positive, negative, exclude, limit, languages, mode, reqId);
  }

  /**
   * Pure similar — TrackPage. Seed = вектор конкретного трека.
   * НЕТ taste юзера, НЕТ лайков/дизлайков/скипов (только клиентский exclude).
   */
  async similar(
    scTrackId: string,
    exclude: string[],
    limit = 10,
    languages?: string[],
    diversity = 0,
  ) {
    const anchorTrackId = parseIdOrNull(scTrackId);
    if (anchorTrackId === null) return [];
    return this.runSimilar({ anchorTrackId, exclude, limit, languages, diversity });
  }

  private async runSimilar(input: SimilarInput): Promise<RecommendResult[]> {
    const div = Math.max(0, Math.min(1, input.diversity));
    const filter = this.buildFilter(input.exclude, input.languages);
    const fetchLimit = Math.max(input.limit * (div > 0.5 ? 12 : 4), div > 0.5 ? 160 : 40);
    const threshold = Math.max(0, this.scoreThreshold - div * 0.25);

    const perBranch = await Promise.all(
      this.branches.map((b) =>
        this.recommendSafe(
          b.trackCollection,
          [input.anchorTrackId],
          [],
          filter,
          fetchLimit,
          undefined,
          threshold,
        ),
      ),
    );
    const fused = this.rrfFuseWeighted(
      perBranch.map((results, i) => ({ results, weight: this.branches[i].weight })),
      fetchLimit,
    );
    const enriched = await this.enrichAndBoost(fused);
    const mmred =
      div > 0
        ? await this.applyMmr(enriched, div, Math.min(enriched.length, input.limit * 8))
        : enriched;
    const cap = div >= 0.5 ? 1 : this.artistCapN;
    const diverse = this.artistCap(mmred, cap);
    return this.takeVerified(diverse, input.limit);
  }

  /** Текстовый поиск аудио через MuQ-MuLan ("грустный трек с гитарой"). */
  async searchByText(query: string, limit = 20, languages?: string[]): Promise<RecommendResult[]> {
    const q = query.trim();
    if (!q) return [];
    let vec: number[] | null;
    try {
      vec = await this.worker.encodeTextMulan(q);
    } catch (e) {
      this.logger.debug(`searchByText: worker encodeTextMulan failed: ${(e as Error).message}`);
      return [];
    }
    if (!vec?.length) return [];

    const filter = this.buildFilter([], languages);
    const fetchLimit = Math.max(limit * 3, 40);

    let results: RecommendResult[];
    try {
      const raw = await this.qdrant.search('tracks_clap', {
        vector: vec,
        filter: filter as Record<string, unknown>,
        limit: fetchLimit,
        with_payload: true,
      });
      results = raw as unknown as RecommendResult[];
    } catch (e) {
      this.logger.debug(`searchByText: qdrant search failed: ${(e as Error).message}`);
      return [];
    }

    const enriched = await this.enrichAndBoost(results);
    const diverse = this.artistCap(enriched, this.artistCapN);
    return this.takeVerified(diverse, limit);
  }

  /**
   * SIMILAR mode — 3 базы вместе. Кандидаты из tracks_mert (taste + anchor seed),
   * скоринг каждого = w_m·cos(track.mert,user_taste.mert) + w_c·cos(track.clap,user_taste.clap)
   *                 + w_l·cos(track.lyrics,user_taste.lyrics).
   * Если у трека/юзера нет clap/lyrics-вектора — компонента = 0.
   * Без MMR — отсортировано по итоговому score, artistCap = N.
   */
  private async waveSimilar(
    scUserId: string,
    anchorTrackId: number | null,
    positiveIds: number[],
    negativeIds: number[],
    exclude: string[],
    limit: number,
    languages?: string[],
    reqId = '-',
  ): Promise<RecommendResult[]> {
    const fetchLimit = Math.max(limit * 12, 300);
    this.logger.log(
      `[${reqId}] waveSimilar() params  anchor=${anchorTrackId ?? 'null'} ` +
        `fetchLimit=${fetchLimit} threshold=${this.scoreThreshold} mmr=OFF artistCap=${this.artistCapN} ` +
        `popBoost=${this.popularityBoost} weights={mert:${this.weightMert},clap:${this.weightClap},lyrics:${this.weightLyrics}}`,
    );

    const scored = await this.candidatesByThreeBases(
      scUserId,
      anchorTrackId,
      positiveIds,
      negativeIds,
      exclude,
      languages,
      fetchLimit,
      this.scoreThreshold,
      reqId,
    );
    const enriched = await this.enrichAndBoost(scored);
    const diverse = this.artistCap(enriched, this.artistCapN);
    this.logger.log(
      `[${reqId}] waveSimilar() pipeline  scored=${scored.length} enriched=${enriched.length} ` +
        `afterArtistCap=${diverse.length} sliced=${Math.min(diverse.length, limit)}`,
    );
    return this.takeVerified(diverse, limit);
  }

  /**
   * DIVERSE mode — 3 базы вместе + MMR поверх. Та же скоринг-механика что и similar,
   * но: пул больше, threshold ниже, после сортировки по комбинированному score —
   * MMR (λ=0.3) разбавляет результат разнообразием в пределах taste-кластера.
   */
  private async waveDiverse(
    scUserId: string,
    anchorTrackId: number | null,
    positiveIds: number[],
    negativeIds: number[],
    exclude: string[],
    limit: number,
    languages?: string[],
    reqId = '-',
  ): Promise<RecommendResult[]> {
    const div = DIVERSE_DIVERSITY;
    const fetchLimit = Math.max(limit * 20, 500);
    const threshold = Math.max(0, this.scoreThreshold - div * 0.4);

    this.logger.log(
      `[${reqId}] waveDiverse() params  anchor=${anchorTrackId ?? 'null'} diversity=${div} ` +
        `(λ=${(1 - div).toFixed(2)}) fetchLimit=${fetchLimit} threshold=${threshold.toFixed(3)} ` +
        `mmr=ON artistCap=${this.artistCapN} popBoost=${this.popularityBoost} ` +
        `weights={mert:${this.weightMert},clap:${this.weightClap},lyrics:${this.weightLyrics}}`,
    );

    const scored = await this.candidatesByThreeBases(
      scUserId,
      anchorTrackId,
      positiveIds,
      negativeIds,
      exclude,
      languages,
      fetchLimit,
      threshold,
      reqId,
    );
    const enriched = await this.enrichAndBoost(scored);
    const mmrWorkLimit = Math.min(enriched.length, Math.max(limit * 8, 120));
    const mmred = await this.applyMmr(enriched, div, mmrWorkLimit);
    const diverse = this.artistCap(mmred, this.artistCapN);
    this.logger.log(
      `[${reqId}] waveDiverse() pipeline  scored=${scored.length} enriched=${enriched.length} ` +
        `mmrWorkLimit=${mmrWorkLimit} afterMmr=${mmred.length} afterArtistCap=${diverse.length} ` +
        `sliced=${Math.min(diverse.length, limit)}`,
    );
    return this.takeVerified(diverse, limit);
  }

  /**
   * Главное ядро 3-base scoring:
   *   1. Кандидаты — из tracks_mert: recommend по user_taste_mert (через lookup_from)
   *      + опционально recommend по anchor (если задан); cold-start — recommend по
   *      positiveIds (последние лайки). Объединяем в пул, по дублям берём max mert score.
   *   2. Достаём векторы user_taste_clap и user_taste_lyrics.
   *   3. Batch-retrieve clap/lyrics векторов всех кандидатов (по одному запросу на коллекцию).
   *   4. final = w_m·mertScore + w_c·cos(track.clap,user_taste.clap) + w_l·cos(...lyrics).
   *      Если у трека/юзера компонента отсутствует — вклад 0.
   *   5. Sort desc.
   */
  private async candidatesByThreeBases(
    scUserId: string,
    anchorTrackId: number | null,
    positiveIds: number[],
    negativeIds: number[],
    exclude: string[],
    languages: string[] | undefined,
    fetchLimit: number,
    threshold: number,
    reqId: string,
  ): Promise<RecommendResult[]> {
    const userTasteId = this.userIdToQdrantId(scUserId);
    const filter = this.buildFilter(exclude, languages);

    // Step 1: достаём user_taste векторы (по 3 базам), параллельно с пулом
    const [userMertVec, userClapVec, userLyricsVec] = await Promise.all([
      this.retrieveVector('user_taste_mert', userTasteId),
      this.retrieveVector('user_taste_clap', userTasteId),
      this.retrieveVector('user_taste_lyrics', userTasteId),
    ]);
    this.logger.log(
      `[${reqId}] 3bases.user-taste  mert=${userMertVec ? 'OK' : 'NULL'} ` +
        `clap=${userClapVec ? 'OK' : 'NULL'} lyrics=${userLyricsVec ? 'OK' : 'NULL'}`,
    );

    // Step 2: пул кандидатов из tracks_mert
    const pool = new Map<string | number, PoolEntry>();
    const mergeIntoPool = (results: RecommendResult[]) => {
      for (const r of results) {
        const score = r.score ?? 0;
        const prev = pool.get(r.id);
        if (!prev || score > prev.mertScore) {
          pool.set(r.id, { id: r.id, mertScore: score, payload: r.payload });
        }
      }
    };

    if (userMertVec) {
      const res = await this.recommendSafe(
        'tracks_mert',
        [userTasteId],
        negativeIds,
        filter,
        fetchLimit,
        'user_taste_mert',
        threshold,
      );
      this.logger.log(`[${reqId}] 3bases.pool taste-arm got=${res.length}`);
      mergeIntoPool(res);
    } else if (positiveIds.length) {
      const res = await this.recommendSafe(
        'tracks_mert',
        positiveIds,
        negativeIds,
        filter,
        fetchLimit,
        undefined,
        threshold,
      );
      this.logger.log(
        `[${reqId}] 3bases.pool positive-arm (cold-start)  seedIds=${positiveIds.length} got=${res.length}`,
      );
      mergeIntoPool(res);
    }

    if (anchorTrackId !== null) {
      const res = await this.recommendSafe(
        'tracks_mert',
        [anchorTrackId],
        negativeIds,
        filter,
        fetchLimit,
        undefined,
        threshold,
      );
      this.logger.log(`[${reqId}] 3bases.pool anchor-arm  seed=${anchorTrackId} got=${res.length}`);
      mergeIntoPool(res);
    }

    if (!pool.size) {
      this.logger.warn(`[${reqId}] 3bases.pool empty`);
      return [];
    }

    // Step 3: batch-retrieve clap/lyrics векторов для всех кандидатов
    const candidates = [...pool.values()];
    const numericIds = candidates
      .map((c) => Number(c.id))
      .filter((n) => Number.isFinite(n)) as number[];

    const [clapVecs, lyricsVecs] = await Promise.all([
      userClapVec
        ? this.retrieveVectors('tracks_clap', numericIds)
        : Promise.resolve(new Map<string, number[]>()),
      userLyricsVec
        ? this.retrieveVectors('tracks_lyrics', numericIds)
        : Promise.resolve(new Map<string, number[]>()),
    ]);
    this.logger.log(
      `[${reqId}] 3bases.vectors  candidates=${candidates.length} ` +
        `clap=${clapVecs.size}/${candidates.length} lyrics=${lyricsVecs.size}/${candidates.length}`,
    );

    // Step 4: combined score
    const wM = this.weightMert;
    const wC = this.weightClap;
    const wL = this.weightLyrics;
    let withClap = 0;
    let withLyrics = 0;

    const scored: RecommendResult[] = candidates.map((c) => {
      const id = String(c.id);
      const clapVec = userClapVec ? clapVecs.get(id) : undefined;
      const lyricsVec = userLyricsVec ? lyricsVecs.get(id) : undefined;
      const cs = userClapVec && clapVec ? this.cosine(clapVec, userClapVec) : 0;
      const ls = userLyricsVec && lyricsVec ? this.cosine(lyricsVec, userLyricsVec) : 0;
      if (clapVec) withClap++;
      if (lyricsVec) withLyrics++;
      const final = wM * c.mertScore + wC * cs + wL * ls;
      return { id: c.id, score: final, payload: c.payload };
    });

    scored.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
    this.logger.log(
      `[${reqId}] 3bases.scored  total=${scored.length} withClap=${withClap} withLyrics=${withLyrics} ` +
        `top3=[${scored
          .slice(0, 3)
          .map((s) => `${s.id}:${(s.score ?? 0).toFixed(3)}`)
          .join(',')}]`,
    );
    return scored;
  }

  private async recommendSafe(
    collection: string,
    positive: (string | number)[],
    negative: number[],
    filter: QdrantFilter | undefined,
    limit: number,
    lookupFrom?: string,
    scoreThreshold?: number,
  ): Promise<RecommendResult[]> {
    try {
      const params: Record<string, unknown> = {
        positive,
        negative: negative.length ? negative : undefined,
        strategy: 'best_score',
        filter,
        limit,
        with_payload: true,
        score_threshold: scoreThreshold ?? this.scoreThreshold,
      };
      if (lookupFrom) params.lookup_from = { collection: lookupFrom };
      const results = await this.qdrant.recommend(collection, params as never);
      return results as unknown as RecommendResult[];
    } catch (e) {
      this.logger.debug(`recommend ${collection} failed: ${(e as Error).message}`);
      return [];
    }
  }

  private async retrieveVector(collection: string, id: number): Promise<number[] | null> {
    try {
      const pts = (await this.qdrant.retrieve(collection, {
        ids: [id],
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ vector?: number[] | null }>;
      const v = pts[0]?.vector;
      return Array.isArray(v) ? v : null;
    } catch {
      return null;
    }
  }

  private async retrieveVectors(collection: string, ids: number[]): Promise<Map<string, number[]>> {
    const out = new Map<string, number[]>();
    if (!ids.length) return out;
    try {
      const pts = (await this.qdrant.retrieve(collection, {
        ids,
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ id: string | number; vector?: number[] | null }>;
      for (const p of pts) {
        if (Array.isArray(p.vector)) out.set(String(p.id), p.vector);
      }
    } catch (e) {
      this.logger.debug(`retrieveVectors ${collection} failed: ${(e as Error).message}`);
    }
    return out;
  }

  private rrfFuseWeighted(
    branches: Array<{ results: RecommendResult[]; weight: number }>,
    limit: number,
  ): RecommendResult[] {
    const acc = new Map<string | number, { score: number; item: RecommendResult }>();
    for (const { results, weight } of branches) {
      results.forEach((item, idx) => {
        const add = weight / (RRF_K + idx + 1);
        const prev = acc.get(item.id);
        acc.set(item.id, { score: (prev?.score ?? 0) + add, item: prev?.item ?? item });
      });
    }
    return [...acc.values()]
      .sort((a, b) => b.score - a.score)
      .slice(0, limit)
      .map(({ score, item }) => ({ ...item, score }));
  }

  private async enrichAndBoost(items: RecommendResult[]): Promise<RecommendResult[]> {
    if (!items.length) return items;
    const ids = items.map((it) => String(it.id));
    const tracks = await this.tracksRepo.find({
      where: { scTrackId: In(ids) },
      select: ['scTrackId', 'rawScData'],
    });
    const byId = new Map(tracks.map((t) => [t.scTrackId, t]));
    const boost = this.popularityBoost;

    return items
      .map((it) => {
        const t = byId.get(String(it.id));
        const raw = (t?.rawScData ?? {}) as {
          publisher_metadata?: { artist?: string };
          user?: { username?: string };
          playback_count?: number;
          genre?: string;
        };
        const artist = raw.publisher_metadata?.artist || raw.user?.username || null;
        const playbackCount = Number(raw.playback_count ?? 0);
        const bonus = Math.log1p(Math.max(0, playbackCount)) * boost;
        return {
          ...it,
          score: (it.score ?? 0) + bonus,
          artist,
          genre: raw.genre ?? null,
          playbackCount,
        };
      })
      .sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
  }

  /**
   * MMR re-rank. `diversity` ∈ (0, 1]: 0 = off, 1 = max variety.
   * λ = 1 - diversity. score = λ·relevance − (1−λ)·max_sim(to_selected).
   */
  private async applyMmr(
    items: RecommendResult[],
    diversity: number,
    workLimit: number,
  ): Promise<RecommendResult[]> {
    if (items.length <= 1) return items;
    const lambda = Math.max(0, Math.min(1, 1 - diversity));

    const top = items.slice(0, workLimit);
    const numericIds = top.map((it) => Number(it.id)).filter((n) => Number.isFinite(n)) as number[];
    if (!numericIds.length) return items;

    let points: Array<{ id: string | number; vector?: number[] | null }>;
    try {
      points = (await this.qdrant.retrieve('tracks_mert', {
        ids: numericIds,
        with_vector: true,
        with_payload: false,
      })) as unknown as Array<{ id: string | number; vector?: number[] | null }>;
    } catch (e) {
      this.logger.debug(`mmr retrieve failed: ${(e as Error).message}`);
      return items;
    }

    const vectors = new Map<string, number[]>();
    for (const p of points) {
      if (Array.isArray(p.vector)) vectors.set(String(p.id), p.vector);
    }
    if (vectors.size < 2) {
      this.logger.warn(
        `applyMmr: only ${vectors.size} vectors retrieved out of ${numericIds.length} ids — MMR skipped, returning items as-is`,
      );
      return items;
    }

    const pool = top.filter((it) => vectors.has(String(it.id)));
    const noVec = top.filter((it) => !vectors.has(String(it.id)));
    pool.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
    this.logger.log(
      `applyMmr: λ=${lambda.toFixed(2)} workLimit=${workLimit} pool=${pool.length} noVec=${noVec.length} vectors=${vectors.size}`,
    );

    const selected: RecommendResult[] = [pool.shift()!];
    while (selected.length < workLimit && pool.length) {
      let bestIdx = 0;
      let bestScore = -Infinity;
      for (let i = 0; i < pool.length; i++) {
        const cand = pool[i];
        const candVec = vectors.get(String(cand.id));
        if (!candVec) continue;
        let maxSim = 0;
        for (const sel of selected) {
          const selVec = vectors.get(String(sel.id));
          if (!selVec) continue;
          const s = this.cosine(candVec, selVec);
          if (s > maxSim) maxSim = s;
        }
        const rel = cand.score ?? 0;
        const mmr = lambda * rel - (1 - lambda) * maxSim;
        if (mmr > bestScore) {
          bestScore = mmr;
          bestIdx = i;
        }
      }
      selected.push(pool.splice(bestIdx, 1)[0]);
    }

    return [...selected, ...noVec, ...items.slice(workLimit)];
  }

  private async takeVerified(items: RecommendResult[], limit: number): Promise<RecommendResult[]> {
    const out: RecommendResult[] = [];
    const batchSize = Math.max(limit, 8);
    for (let i = 0; i < items.length && out.length < limit; i += batchSize) {
      const slice = items.slice(i, i + batchSize);
      const ids = slice.map((s) => String(s.id));
      const missing = await this.s3.findMissing(ids);
      for (const item of slice) {
        if (out.length >= limit) break;
        if (!missing.has(String(item.id))) out.push(item);
      }
    }
    return out;
  }

  private cosine(a: number[], b: number[]): number {
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

  private artistCap(items: RecommendResult[], cap: number): RecommendResult[] {
    if (cap <= 0) return items;
    const counts = new Map<string, number>();
    const out: RecommendResult[] = [];
    for (const it of items) {
      const key = (it.artist || String(it.id)).toLowerCase();
      const n = counts.get(key) ?? 0;
      if (n >= cap) continue;
      counts.set(key, n + 1);
      out.push(it);
    }
    return out;
  }

  private buildFilter(exclude: string[], languages?: string[]): QdrantFilter | undefined {
    const filter: QdrantFilter = {};
    if (exclude.length) {
      filter.must_not = exclude.map((id) => ({
        key: 'sc_track_id',
        match: { value: id },
      }));
    }
    if (languages?.length) {
      filter.must = [{ key: 'language', match: { any: languages } }];
    }
    return Object.keys(filter).length ? filter : undefined;
  }

  private async getFallbackTracks(exclude: string[], limit: number, languages?: string[]) {
    const where: Record<string, unknown> = { indexedAt: Not(IsNull()) };
    if (languages?.length) {
      where.language = In(languages);
    }
    const tracks = await this.tracksRepo.find({
      where,
      order: { indexedAt: 'DESC' },
      take: Math.max(limit * 3, 60),
      select: ['scTrackId'],
    });
    return tracks
      .filter((t) => !exclude.includes(t.scTrackId))
      .slice(0, limit)
      .map((t) => ({ id: t.scTrackId, payload: { sc_track_id: t.scTrackId } }));
  }

  private userIdToQdrantId(userId: string): number {
    const hash = createHash('sha256').update(userId).digest();
    return Number(hash.readBigUInt64BE(0) % BigInt(Number.MAX_SAFE_INTEGER));
  }
}
