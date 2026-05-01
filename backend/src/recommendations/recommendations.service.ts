import { Inject, Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { QdrantClient } from '@qdrant/js-client-rest';
import { In, IsNull, Not, Repository } from 'typeorm';
import { CentroidService } from '../centroids/centroid.service.js';
import { CollabVectorService } from '../collab/collab-vector.service.js';
import { userIdToQdrantId } from '../common/user-id.js';
import { IndexedTrack } from '../indexing/entities/indexed-track.entity.js';
import { LTR_FEATURE_COUNT, LtrService } from '../ltr/ltr.service.js';
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
  /** Внутреннее: фичи для LTR (через enrichAndBoost). */
  features?: number[];
}

interface SeedVectors {
  collab: number[] | null;
  mert: number[] | null;
  clap: number[] | null;
  lyrics: number[] | null;
}

interface ScoredCandidate {
  id: string | number;
  score: number;
  payload?: Record<string, unknown>;
  /** Сырые компоненты — используются как фичи для LTR. */
  features?: number[];
}

const DIVERSE_DIVERSITY = 0.7;

export type WaveMode = 'similar' | 'diverse';

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
    private readonly centroids: CentroidService,
    private readonly collab: CollabVectorService,
    private readonly ltr: LtrService,
  ) {}

  /* ── Конфиг через ENV ─────────────────────────────────────── */

  /** Collab (item2vec) — primary signal. Высокий вес, потому что behavioral. */
  private get weightCollab() {
    return Number.parseFloat(process.env.SOUNDWAVE_COLLAB_WEIGHT ?? '0.55');
  }
  private get weightMert() {
    return Number.parseFloat(process.env.SOUNDWAVE_AUDIO_WEIGHT ?? '0.20');
  }
  private get weightClap() {
    return Number.parseFloat(process.env.SOUNDWAVE_CLAP_WEIGHT ?? '0.10');
  }
  private get weightLyrics() {
    return Number.parseFloat(process.env.SOUNDWAVE_LYRICS_WEIGHT ?? '0.15');
  }
  private get popularityBoost() {
    return Number.parseFloat(process.env.SOUNDWAVE_POPULARITY_BOOST ?? '0');
  }
  private get artistCapN() {
    return Number.parseInt(process.env.SOUNDWAVE_ARTIST_CAP ?? '2', 10);
  }
  /** Whitened-cos threshold для отсечения слабых кандидатов. После whitening scale меньше. */
  private get scoreThreshold() {
    return Number.parseFloat(process.env.SOUNDWAVE_SCORE_THRESHOLD ?? '0.05');
  }

  /* ── Public API ───────────────────────────────────────────── */

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

    const div = mode === 'diverse' ? DIVERSE_DIVERSITY : 0;
    const fetchLimit = mode === 'diverse' ? Math.max(limit * 20, 500) : Math.max(limit * 12, 300);
    const threshold = Math.max(0, this.scoreThreshold - div * 0.04);

    const userTasteId = userIdToQdrantId(scUserId);
    const [taste, userCollab] = await Promise.all([
      this.loadUserTasteVectors(userTasteId),
      this.collab.getUserVector(scUserId),
    ]);
    const seed: SeedVectors = {
      collab: userCollab,
      mert: taste.mert,
      clap: taste.clap,
      lyrics: taste.lyrics,
    };

    this.logger.log(
      `[${reqId}] wave start mode=${mode} anchor=${anchorTrackId ?? 'null'} pos=${positiveIds.length} ` +
        `neg=${negativeIds.length} excl=${exclude.length} limit=${limit} fetch=${fetchLimit} thr=${threshold.toFixed(3)} ` +
        `seed: collab=${seed.collab ? 'OK' : 'NULL'} mert=${seed.mert ? 'OK' : 'NULL'} ` +
        `clap=${seed.clap ? 'OK' : 'NULL'} lyrics=${seed.lyrics ? 'OK' : 'NULL'} ` +
        `w={col:${this.weightCollab},m:${this.weightMert},c:${this.weightClap},l:${this.weightLyrics}}`,
    );

    const candidateIds = await this.buildCandidatePool({
      userCollab,
      seedHasTaste: !!seed.mert,
      userTasteId,
      anchorTrackId,
      positiveIds,
      negativeIds,
      exclude,
      languages,
      fetchLimit,
      reqId,
    });
    if (!candidateIds.length) {
      this.logger.warn(`[${reqId}] wave: empty pool, fallback`);
      return this.getFallbackTracks(exclude, limit, languages);
    }

    const scored = await this.scoreByAllBases(candidateIds, seed, reqId);
    const filtered = scored.filter((s) => s.score >= threshold);
    this.logger.log(
      `[${reqId}] wave scored=${scored.length} afterThr=${filtered.length} ` +
        `top3=[${filtered
          .slice(0, 3)
          .map((s) => `${s.id}:${s.score.toFixed(3)}`)
          .join(',')}]`,
    );

    const enriched = await this.enrichAndBoost(filtered, languages);
    const reranked = await this.applyLtrRerank(
      enriched,
      Math.min(enriched.length, limit * 4),
      reqId,
    );
    let ranked = reranked;
    if (div > 0) {
      ranked = await this.applyMmr(reranked, div, Math.min(reranked.length, limit * 8));
    }
    const diverse = this.artistCap(ranked, this.artistCapN);
    const verified = await this.takeVerified(diverse, limit);
    if (verified.length >= 5) return verified;

    this.logger.warn(`[${reqId}] wave too few results (${verified.length}), fallback`);
    return this.getFallbackTracks(exclude, limit, languages);
  }

  /** Wrapper для feed без anchor. */
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
   * "Похожие на трек" — то же 3-base scoring что и wave, но seed = вектора самого трека
   * (без учёта вкуса юзера). Используется на TrackPage и для autoplay queue.
   * Принимает опциональный excludeDisliked — список треков юзера для фильтра must_not.
   */
  async similar(
    scTrackId: string,
    exclude: string[],
    limit = 10,
    languages?: string[],
    diversity = 0,
    reqId = '-',
  ): Promise<RecommendResult[]> {
    const anchorTrackId = parseIdOrNull(scTrackId);
    if (anchorTrackId === null) return [];

    const div = Math.max(0, Math.min(1, diversity));
    const fetchLimit = Math.max(limit * (div > 0.5 ? 18 : 8), div > 0.5 ? 240 : 80);
    const threshold = Math.max(0, this.scoreThreshold - div * 0.04);

    const seed = await this.loadTrackVectors(anchorTrackId);
    if (!seed.collab && !seed.mert && !seed.clap && !seed.lyrics) {
      this.logger.warn(`[${reqId}] similar: anchor ${anchorTrackId} has no vectors`);
      return [];
    }

    this.logger.log(
      `[${reqId}] similar start anchor=${anchorTrackId} div=${div} limit=${limit} fetch=${fetchLimit} ` +
        `seed collab=${seed.collab ? 'OK' : '-'} mert=${seed.mert ? 'OK' : '-'} ` +
        `clap=${seed.clap ? 'OK' : '-'} lyrics=${seed.lyrics ? 'OK' : '-'}`,
    );

    const filter = this.buildFilter(exclude, languages);
    const pool = new Set<number>();

    // Retrieval armы: collab (если есть в vocab) + audio backup. Объединяем.
    const tasks: Promise<void>[] = [];
    if (seed.collab) {
      tasks.push(
        this.searchByVector('tracks_collab', seed.collab, filter, fetchLimit).then((res) => {
          for (const r of res) {
            const n = Number(r.id);
            if (Number.isFinite(n) && n !== anchorTrackId) pool.add(n);
          }
          this.logger.log(`[${reqId}] similar collab-arm got=${res.length}`);
        }),
      );
    }
    tasks.push(
      this.recommendByPositive('tracks_mert', [anchorTrackId], filter, fetchLimit).then((res) => {
        for (const r of res) {
          const n = Number(r.id);
          if (Number.isFinite(n) && n !== anchorTrackId) pool.add(n);
        }
        this.logger.log(`[${reqId}] similar audio-arm got=${res.length}`);
      }),
    );
    await Promise.all(tasks);
    const candidateIds = [...pool];
    if (!candidateIds.length) {
      this.logger.warn(`[${reqId}] similar: empty pool`);
      return [];
    }

    const scored = await this.scoreByAllBases(candidateIds, seed, reqId);
    const filtered = scored.filter((s) => s.score >= threshold);
    const enriched = await this.enrichAndBoost(filtered, languages);
    const reranked = await this.applyLtrRerank(
      enriched,
      Math.min(enriched.length, limit * 4),
      reqId,
    );

    let ranked = reranked;
    if (div > 0) {
      ranked = await this.applyMmr(reranked, div, Math.min(reranked.length, limit * 8));
    }
    const cap = div >= 0.5 ? 1 : this.artistCapN;
    const diverse = this.artistCap(ranked, cap);
    return this.takeVerified(diverse, limit);
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
    let raw: RecommendResult[];
    try {
      raw = (await this.qdrant.search('tracks_clap', {
        vector: vec,
        filter: filter as Record<string, unknown>,
        limit: fetchLimit,
        with_payload: true,
      })) as unknown as RecommendResult[];
    } catch (e) {
      this.logger.debug(`searchByText: qdrant search failed: ${(e as Error).message}`);
      return [];
    }

    const enriched = await this.enrichAndBoost(
      raw.map((r) => ({ id: r.id, score: r.score ?? 0, payload: r.payload })),
      languages,
    );
    const diverse = this.artistCap(enriched, this.artistCapN);
    return this.takeVerified(diverse, limit);
  }

  /* ── Ядро scoring ─────────────────────────────────────────── */

  /**
   * Универсальный скоринг кандидатов против seed-векторов всех 4 баз:
   *   final = w_col·cos(track.collab, seed.collab)             — primary behavioral
   *         + w_m·whitened_cos(track.mert, seed.mert)
   *         + w_c·whitened_cos(track.clap, seed.clap)
   *         + w_l·cos(track.lyrics, seed.lyrics)
   * Whitening (вычитание центроида) применяется только к mert/clap.
   * Если у трека или seed нет компоненты — её вклад = 0.
   *
   * Если у трека есть collab-вектор — он несёт основной сигнал. Аудио-базы
   * выступают как tie-break для треков на границе collab-кластера и cold-start
   * (новые треки которых нет в item2vec vocab).
   */
  private async scoreByAllBases(
    candidateIds: number[],
    seed: SeedVectors,
    reqId: string,
  ): Promise<ScoredCandidate[]> {
    const [collabMap, mertMap, clapMap, lyricsMap, payloadMap] = await Promise.all([
      seed.collab
        ? this.collab.getTrackVectors(candidateIds)
        : Promise.resolve(new Map<string, number[]>()),
      seed.mert
        ? this.retrieveVectors('tracks_mert', candidateIds)
        : Promise.resolve(new Map<string, number[]>()),
      seed.clap
        ? this.retrieveVectors('tracks_clap', candidateIds)
        : Promise.resolve(new Map<string, number[]>()),
      seed.lyrics
        ? this.retrieveVectors('tracks_lyrics', candidateIds)
        : Promise.resolve(new Map<string, number[]>()),
      this.retrievePayloads('tracks_mert', candidateIds),
    ]);

    const cMert = this.centroids.get('tracks_mert');
    const cClap = this.centroids.get('tracks_clap');
    const wCol = this.weightCollab;
    const wM = this.weightMert;
    const wC = this.weightClap;
    const wL = this.weightLyrics;

    let withCollab = 0;
    const out: ScoredCandidate[] = candidateIds.map((id) => {
      const key = String(id);
      const tcol = collabMap.get(key);
      const tm = mertMap.get(key);
      const tc = clapMap.get(key);
      const tl = lyricsMap.get(key);
      const sCol = seed.collab && tcol ? cosine(tcol, seed.collab) : 0;
      const sM = seed.mert && tm ? this.centroids.whitenedCosine(tm, seed.mert, cMert) : 0;
      const sC = seed.clap && tc ? this.centroids.whitenedCosine(tc, seed.clap, cClap) : 0;
      const sL = seed.lyrics && tl ? cosine(tl, seed.lyrics) : 0;
      if (tcol) withCollab++;
      const score = wCol * sCol + wM * sM + wC * sC + wL * sL;
      // Фичи для LTR (последние 2 будут заполнены в enrichAndBoost из payload).
      const features = new Array<number>(LTR_FEATURE_COUNT).fill(0);
      features[0] = sCol;
      features[1] = sM;
      features[2] = sC;
      features[3] = sL;
      return { id, score, payload: payloadMap.get(key), features };
    });

    out.sort((a, b) => b.score - a.score);
    this.logger.log(
      `[${reqId}] scored ${out.length} candidates (withCollab=${withCollab}): top3=[${out
        .slice(0, 3)
        .map((s) => `${s.id}:${s.score.toFixed(3)}`)
        .join(',')}]`,
    );
    return out;
  }

  /* ── Сбор пула кандидатов ─────────────────────────────────── */

  private async buildCandidatePool(args: {
    userCollab: number[] | null;
    seedHasTaste: boolean;
    userTasteId: number;
    anchorTrackId: number | null;
    positiveIds: number[];
    negativeIds: number[];
    exclude: string[];
    languages?: string[];
    fetchLimit: number;
    reqId: string;
  }): Promise<number[]> {
    const filter = this.buildFilter(args.exclude, args.languages);
    const pool = new Set<number>();
    const tasks: Promise<void>[] = [];

    // 1. Primary arm: search в tracks_collab по user_collab вектору. Behavioral сигнал.
    if (args.userCollab) {
      tasks.push(
        this.searchByVector('tracks_collab', args.userCollab, filter, args.fetchLimit).then(
          (res) => {
            for (const r of res) {
              const n = Number(r.id);
              if (Number.isFinite(n)) pool.add(n);
            }
            this.logger.log(`[${args.reqId}] pool collab-arm got=${res.length}`);
          },
        ),
      );
    }

    // 2. Audio taste arm — для cold-start треков (не в item2vec vocab) и tie-break.
    if (args.seedHasTaste) {
      tasks.push(
        this.recommendByLookup(
          'tracks_mert',
          [args.userTasteId],
          args.negativeIds,
          'user_taste_mert',
          filter,
          args.fetchLimit,
        ).then((res) => {
          for (const r of res) {
            const n = Number(r.id);
            if (Number.isFinite(n)) pool.add(n);
          }
          this.logger.log(`[${args.reqId}] pool taste-arm got=${res.length}`);
        }),
      );
    } else if (args.positiveIds.length && !args.userCollab) {
      // 3. Cold-start (нет ни collab, ни taste): рекомендации по последним лайкам.
      tasks.push(
        this.recommendByPositive(
          'tracks_mert',
          args.positiveIds,
          filter,
          args.fetchLimit,
          args.negativeIds,
        ).then((res) => {
          for (const r of res) {
            const n = Number(r.id);
            if (Number.isFinite(n)) pool.add(n);
          }
          this.logger.log(`[${args.reqId}] pool cold-start-arm got=${res.length}`);
        }),
      );
    }

    // 4. Anchor arm (если задан конкретный трек как seed для wave).
    if (args.anchorTrackId !== null) {
      tasks.push(
        this.recommendByPositive(
          'tracks_mert',
          [args.anchorTrackId],
          filter,
          args.fetchLimit,
          args.negativeIds,
        ).then((res) => {
          for (const r of res) {
            const n = Number(r.id);
            if (Number.isFinite(n) && n !== args.anchorTrackId) pool.add(n);
          }
          this.logger.log(`[${args.reqId}] pool anchor-arm got=${res.length}`);
        }),
      );
    }

    await Promise.all(tasks);
    return [...pool];
  }

  /* ── Qdrant helpers ───────────────────────────────────────── */

  private async loadUserTasteVectors(userTasteId: number): Promise<{
    mert: number[] | null;
    clap: number[] | null;
    lyrics: number[] | null;
  }> {
    const [mert, clap, lyrics] = await Promise.all([
      this.retrieveVector('user_taste_mert', userTasteId),
      this.retrieveVector('user_taste_clap', userTasteId),
      this.retrieveVector('user_taste_lyrics', userTasteId),
    ]);
    return { mert, clap, lyrics };
  }

  private async loadTrackVectors(trackId: number): Promise<SeedVectors> {
    const [collab, mert, clap, lyrics] = await Promise.all([
      this.collab.getTrackVector(trackId),
      this.retrieveVector('tracks_mert', trackId),
      this.retrieveVector('tracks_clap', trackId),
      this.retrieveVector('tracks_lyrics', trackId),
    ]);
    return { collab, mert, clap, lyrics };
  }

  /** Прямой Qdrant search по вектору (используется для collab-коллекции). */
  private async searchByVector(
    collection: string,
    vector: number[],
    filter: QdrantFilter | undefined,
    limit: number,
  ): Promise<RecommendResult[]> {
    try {
      const results = await this.qdrant.search(collection, {
        vector,
        filter: filter as Record<string, unknown>,
        limit,
        with_payload: true,
      });
      return results as unknown as RecommendResult[];
    } catch (e) {
      this.logger.debug(`searchByVector ${collection} failed: ${(e as Error).message}`);
      return [];
    }
  }

  private async recommendByPositive(
    collection: string,
    positive: number[],
    filter: QdrantFilter | undefined,
    limit: number,
    negative: number[] = [],
  ): Promise<RecommendResult[]> {
    try {
      const results = await this.qdrant.recommend(collection, {
        positive,
        negative: negative.length ? negative : undefined,
        strategy: 'best_score',
        filter,
        limit,
        with_payload: true,
      } as never);
      return results as unknown as RecommendResult[];
    } catch (e) {
      this.logger.debug(`recommendByPositive ${collection} failed: ${(e as Error).message}`);
      return [];
    }
  }

  private async recommendByLookup(
    collection: string,
    positive: number[],
    negative: number[],
    lookupFrom: string,
    filter: QdrantFilter | undefined,
    limit: number,
  ): Promise<RecommendResult[]> {
    try {
      const results = await this.qdrant.recommend(collection, {
        positive,
        negative: negative.length ? negative : undefined,
        strategy: 'best_score',
        filter,
        limit,
        with_payload: true,
        lookup_from: { collection: lookupFrom },
      } as never);
      return results as unknown as RecommendResult[];
    } catch (e) {
      this.logger.debug(`recommendByLookup ${collection} failed: ${(e as Error).message}`);
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

  private async retrievePayloads(
    collection: string,
    ids: number[],
  ): Promise<Map<string, Record<string, unknown>>> {
    const out = new Map<string, Record<string, unknown>>();
    if (!ids.length) return out;
    try {
      const pts = (await this.qdrant.retrieve(collection, {
        ids,
        with_vector: false,
        with_payload: true,
      })) as unknown as Array<{ id: string | number; payload?: Record<string, unknown> | null }>;
      for (const p of pts) {
        if (p.payload) out.set(String(p.id), p.payload);
      }
    } catch (e) {
      this.logger.debug(`retrievePayloads ${collection} failed: ${(e as Error).message}`);
    }
    return out;
  }

  /* ── Enrichment / re-ranking / verification ───────────────── */

  private async enrichAndBoost(
    items: ScoredCandidate[],
    userLanguages?: string[],
  ): Promise<RecommendResult[]> {
    if (!items.length) return [];
    const ids = items.map((it) => String(it.id));
    const tracks = await this.tracksRepo.find({
      where: { scTrackId: In(ids) },
      select: ['scTrackId', 'rawScData', 'language'],
    });
    const byId = new Map(tracks.map((t) => [t.scTrackId, t]));
    const boost = this.popularityBoost;
    const userLangSet = new Set(userLanguages ?? []);

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
        // Дозаполняем LTR-фичи: log_playback + language_match.
        const features = it.features
          ? [...it.features]
          : new Array<number>(LTR_FEATURE_COUNT).fill(0);
        features[4] = Math.log1p(Math.max(0, playbackCount));
        features[5] = t?.language && userLangSet.has(t.language) ? 1.0 : 0.0;
        return {
          id: it.id,
          score: it.score + bonus,
          payload: it.payload,
          artist,
          genre: raw.genre ?? null,
          playbackCount,
          features,
        };
      })
      .sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
  }

  /**
   * LTR rerank: отдаём фичи воркеру (LightGBM ranker), получаем скоры,
   * заменяем `score` и пересортировываем. Если воркер недоступен / модель
   * не обучена — оставляем линейный score (он же fallback на воркере).
   * Применяется только к топу (workLimit), нижний хвост оставляем как есть —
   * экономим RPC payload и не теряем порядок «слабых» кандидатов.
   */
  private async applyLtrRerank(
    items: RecommendResult[],
    workLimit: number,
    reqId: string,
  ): Promise<RecommendResult[]> {
    if (!this.ltr.enabled || items.length <= 1) return items;
    const head = items.slice(0, workLimit);
    const tail = items.slice(workLimit);
    const features = head.map((it) => it.features ?? new Array<number>(LTR_FEATURE_COUNT).fill(0));
    const scores = await this.ltr.score(features);
    if (!scores) return items;

    const reranked = head.map((it, i) => ({ ...it, score: scores[i] }));
    reranked.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));
    this.logger.log(
      `[${reqId}] ltr-rerank applied to ${reranked.length} items, top3=[${reranked
        .slice(0, 3)
        .map((r) => `${r.id}:${(r.score ?? 0).toFixed(3)}`)
        .join(',')}]`,
    );
    return [...reranked, ...tail];
  }

  /** MMR re-rank на whitened mert vectors. λ = 1 - diversity. */
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

    const vectors = await this.retrieveVectors('tracks_mert', numericIds);
    if (vectors.size < 2) return items;

    const cMert = this.centroids.get('tracks_mert');
    const whiten = (v: number[]) => (cMert ? subtract(v, cMert) : v);

    const pool = top.filter((it) => vectors.has(String(it.id)));
    const noVec = top.filter((it) => !vectors.has(String(it.id)));
    pool.sort((a, b) => (b.score ?? 0) - (a.score ?? 0));

    const selected: RecommendResult[] = [pool.shift()!];
    while (selected.length < workLimit && pool.length) {
      let bestIdx = 0;
      let bestScore = -Infinity;
      for (let i = 0; i < pool.length; i++) {
        const cand = pool[i];
        const candVec = whiten(vectors.get(String(cand.id))!);
        let maxSim = 0;
        for (const sel of selected) {
          const selVec = whiten(vectors.get(String(sel.id))!);
          const s = cosine(candVec, selVec);
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

  private async getFallbackTracks(
    exclude: string[],
    limit: number,
    languages?: string[],
  ): Promise<RecommendResult[]> {
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
}

/* ── Pure helpers ─────────────────────────────────────────── */

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

function subtract(a: number[], b: number[]): number[] {
  const n = Math.min(a.length, b.length);
  const out = new Array<number>(n);
  for (let i = 0; i < n; i++) out[i] = a[i] - b[i];
  return out;
}
