import { Inject, Injectable, Logger, OnModuleDestroy, OnModuleInit } from '@nestjs/common';
import { and, asc, eq, isNotNull, isNull, lt, sql } from 'drizzle-orm';
import { NatsService } from '../bus/nats.service.js';
import { STREAMS, SUBJECTS } from '../bus/subjects.js';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { indexedTracks, type LyricsCache, lyricsCache, type NewLyricsCache } from '../db/schema.js';
import { TranscodeTriggerService } from '../transcode/transcode-trigger.service.js';
import { GeniusService } from './genius.service.js';
import { LrclibService } from './lrclib.service.js';
import {
  canonMeta,
  detectLanguageHeuristic,
  heuristicQueries,
  pickLyricsText,
  stripLrcTimestamps,
} from './lyrics.util.js';
import { MusixmatchService } from './musixmatch.service.js';
import { NeteaseService } from './netease.service.js';
import { WorkerClient } from './worker.client.js';

export type LyricsSource = LyricsCache['source'];

export interface LyricsResponse {
  scTrackId: string | null;
  syncedLrc: string | null;
  plainText: string | null;
  source: LyricsSource;
  language: string | null;
  languageConfidence: number | null;
}

export interface LyricsHints {
  title?: string;
  artist?: string;
  durationSec?: number;
}

interface Candidate {
  source: LyricsSource;
  syncedLrc: string | null;
  plainText: string | null;
  artistGuess?: string;
  titleGuess?: string;
  durationSec?: number;
}

const MIN_RANK_SCORE = 6;
const MAX_CANDIDATES = 8;
const SNIPPET_LEN = 220;
const MIN_META_OVERLAP = 0.25;
const MAX_DURATION_DIFF = 0.25;

const INDEXING_CONCURRENCY = Number.parseInt(process.env.LYRICS_INDEXING_CONCURRENCY ?? '3', 10);

const REAP_INTERVAL_MS = 10 * 60 * 1000;
const REAP_MIN_AGE_MS = 10 * 60 * 1000;
const REAP_LIMIT_ALIGN = 30;
const REAP_LIMIT_FULL = 20;

class Semaphore {
  private queue: Array<() => void> = [];
  private inflight = 0;
  constructor(private readonly max: number) {}
  async run<T>(fn: () => Promise<T>): Promise<T> {
    if (this.inflight >= this.max) {
      await new Promise<void>((resolve) => this.queue.push(resolve));
    }
    this.inflight++;
    try {
      return await fn();
    } finally {
      this.inflight--;
      const next = this.queue.shift();
      if (next) next();
    }
  }
}

const STOPWORDS = new Set([
  'feat',
  'ft',
  'featuring',
  'prod',
  'remix',
  'edit',
  'version',
  'mix',
  'cover',
  'live',
  'acoustic',
  'instrumental',
  'original',
  'official',
  'audio',
  'video',
  'lyrics',
  'lyric',
  'sped',
  'slowed',
  'nightcore',
  'reverb',
  'extended',
  'radio',
  'clean',
  'explicit',
  'hd',
  'hq',
  'mv',
]);

function tokenize(s: string): Set<string> {
  const lowered = (s ?? '')
    .toLowerCase()
    .replace(/\[[^\]]*]|\([^)]*\)/g, ' ')
    .replace(/[^\p{L}\p{N}\s]/gu, ' ');
  const out = new Set<string>();
  for (const t of lowered.split(/\s+/)) {
    if (t.length < 2 || STOPWORDS.has(t)) continue;
    out.add(t);
  }
  return out;
}

function metaOverlap(src: string, cand: string): number {
  const a = tokenize(src);
  const b = tokenize(cand);
  if (!a.size || !b.size) return 0;
  let common = 0;
  for (const t of a) if (b.has(t)) common++;
  return common / Math.min(a.size, b.size);
}

function normalizeScTrackId(raw: string): string {
  const s = String(raw ?? '').trim();
  const idx = s.lastIndexOf(':');
  return idx >= 0 ? s.slice(idx + 1) : s;
}

@Injectable()
export class LyricsService implements OnModuleInit, OnModuleDestroy {
  private readonly logger = new Logger(LyricsService.name);
  private readonly inflight = new Map<string, Promise<LyricsResponse>>();
  private readonly whisperInflight = new Map<string, Promise<void>>();
  private readonly indexingSem = new Semaphore(INDEXING_CONCURRENCY);
  private reapTimer?: NodeJS.Timeout;

  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly nats: NatsService,
    private readonly lrclib: LrclibService,
    private readonly mxm: MusixmatchService,
    private readonly genius: GeniusService,
    private readonly netease: NeteaseService,
    private readonly worker: WorkerClient,
    private readonly trigger: TranscodeTriggerService,
  ) {}

  async onModuleInit(): Promise<void> {
    await this.nats.consume(
      STREAMS.done.name,
      'backend-done-embed-lyrics',
      async (data) => {
        const payload = data as { sc_track_id?: string; skipped?: boolean };
        if (!payload.sc_track_id || payload.skipped) return;
        await this.db
          .update(lyricsCache)
          .set({ embeddedAt: sql`now()` })
          .where(
            and(eq(lyricsCache.scTrackId, payload.sc_track_id), isNull(lyricsCache.embeddedAt)),
          );
      },
      SUBJECTS.doneEmbedLyrics,
    );
    this.reapTimer = setInterval(() => {
      this.reapWhisper().catch(() => {});
      this.reapEmbeds().catch(() => {});
    }, REAP_INTERVAL_MS);
  }

  onModuleDestroy(): void {
    if (this.reapTimer) clearInterval(this.reapTimer);
  }

  async ensureLyrics(scTrackIdRaw: string): Promise<LyricsResponse> {
    const scTrackId = normalizeScTrackId(scTrackIdRaw) ?? scTrackIdRaw;
    const cached = await this.db.query.lyricsCache.findFirst({
      where: eq(lyricsCache.scTrackId, scTrackId),
    });
    if (cached) {
      if (!cached.embeddedAt) {
        const text = pickLyricsText(cached.plainText, cached.syncedLrc);
        if (text && text.length > 30) {
          this.afterFound(cached, text).catch((e) =>
            this.logger.warn(`re-embed retry ${scTrackId}: ${(e as Error).message}`),
          );
        }
      }
      return this.toResponse(cached);
    }

    const existing = this.inflight.get(scTrackId);
    if (existing) return existing;

    const promise = (async () => {
      try {
        const hints = await this.loadHintsFromDb(scTrackId);
        return await this.runPipeline(scTrackId, hints, true);
      } finally {
        this.inflight.delete(scTrackId);
      }
    })();
    this.inflight.set(scTrackId, promise);
    return promise;
  }

  async searchLyrics(hints: LyricsHints): Promise<LyricsResponse> {
    if (!hints.title || !hints.artist) return this.emptyResponse(null);
    return this.runPipeline(null, hints as Required<LyricsHints>, false);
  }

  async ensureLyricsForIndexing(scTrackIdRaw: string): Promise<void> {
    const scTrackId = normalizeScTrackId(scTrackIdRaw);
    if (!scTrackId) return;
    try {
      await this.indexingSem.run(() => this.ensureLyrics(scTrackId));
    } catch (e) {
      this.logger.debug(`ensureLyricsForIndexing ${scTrackId}: ${(e as Error).message}`);
    }
  }

  private async runPipeline(
    scTrackId: string | null,
    hints: LyricsHints,
    allowSave: boolean,
  ): Promise<LyricsResponse> {
    const artist = (hints.artist ?? '').trim();
    const title = (hints.title ?? '').trim();
    const durationSec = hints.durationSec ?? 0;
    const logId = scTrackId ?? `${artist} - ${title}`;

    if (!title) {
      this.logger.warn(`lyrics ${logId}: empty title, skip`);
      return this.emptyResponse(scTrackId);
    }

    const picked = await this.findLyrics(logId, artist, title, durationSec);

    if (!picked.plainText && !picked.syncedLrc) {
      this.logger.log(`no lyrics found for ${logId} (${artist} - ${title}) — not caching`);
      return this.emptyResponse(scTrackId);
    }

    if (!allowSave || !scTrackId) {
      return {
        scTrackId,
        syncedLrc: picked.syncedLrc,
        plainText: picked.plainText,
        source: picked.source,
        language: null,
        languageConfidence: null,
      };
    }

    const insertValues: NewLyricsCache = {
      scTrackId,
      syncedLrc: picked.syncedLrc,
      plainText: picked.plainText,
      source: picked.source,
      language: null,
      languageConfidence: null,
      embeddedAt: null,
    };
    const [entity] = await this.db.insert(lyricsCache).values(insertValues).returning();

    const textForLang = pickLyricsText(picked.plainText, picked.syncedLrc);
    if (textForLang && textForLang.length > 30) {
      this.afterFound(entity, textForLang).catch((e) =>
        this.logger.warn(`after-found ${scTrackId}: ${(e as Error).message}`),
      );
    }

    return this.toResponse(entity);
  }

  private async loadHintsFromDb(scTrackId: string): Promise<LyricsHints> {
    const row = await this.db.query.indexedTracks.findFirst({
      where: eq(indexedTracks.scTrackId, scTrackId),
      columns: { title: true, durationMs: true, rawScData: true },
    });
    const raw = (row?.rawScData ?? {}) as {
      title?: string;
      duration?: number;
      publisher_metadata?: { artist?: string };
      user?: { username?: string };
    };
    const title = row?.title ?? raw.title ?? '';
    const artist = raw.publisher_metadata?.artist ?? raw.user?.username ?? '';
    const durMs = row?.durationMs ?? raw.duration ?? 0;
    return {
      title,
      artist,
      durationSec: durMs ? Math.round(durMs / 1000) : 0,
    };
  }

  private emptyResponse(scTrackId: string | null): LyricsResponse {
    return {
      scTrackId,
      syncedLrc: null,
      plainText: null,
      source: 'none',
      language: null,
      languageConfidence: null,
    };
  }

  private async findLyrics(
    logId: string,
    artist: string,
    title: string,
    durationSec: number | undefined,
  ): Promise<Candidate> {
    this.logger.log(
      `findLyrics ${logId}: artist="${artist}" title="${title}" durationSec=${durationSec ?? '?'}`,
    );

    const heuristics = heuristicQueries(artist, title);
    this.logger.log(`[stage1] queries for ${logId}: ${JSON.stringify(heuristics)}`);
    const pick1 = await this.searchAndPick(
      heuristics,
      artist,
      title,
      durationSec,
      logId,
      '[stage1]',
    );
    if (pick1) return pick1;

    const llmQueries = await this.worker.generateSearchQueries(artist, title);
    this.logger.log(`[stage2] queries for ${logId}: ${JSON.stringify(llmQueries)}`);
    const newQueries = llmQueries.filter((q) => !heuristics.includes(q.toLowerCase().trim()));
    if (newQueries.length) {
      const pick2 = await this.searchAndPick(
        llmQueries,
        artist,
        title,
        durationSec,
        logId,
        '[stage2]',
      );
      if (pick2) return pick2;
    } else {
      this.logger.log(`[stage2] LLM added nothing new for ${logId}, skipping fanout`);
    }

    return { source: 'none', syncedLrc: null, plainText: null };
  }

  private async searchAndPick(
    queries: string[],
    artist: string,
    title: string,
    durationSec: number | undefined,
    scTrackId: string,
    stage: string,
  ): Promise<Candidate | null> {
    const raw = await this.fanoutSearch(queries);
    this.logger.log(
      `${stage} candidates for ${scTrackId}: ${raw.length} raw ` +
        `(${raw.map((c) => `${c.source}:${c.artistGuess ?? '?'}-${c.titleGuess ?? '?'}`).join(', ')})`,
    );
    const candidates = this.filterByMetadata(raw, artist, title, durationSec, scTrackId);
    if (!candidates.length) {
      this.logger.log(`${stage} no candidates survived metadata filter for ${scTrackId}`);
      return null;
    }
    const exact = this.pickExactMatch(candidates, queries, artist, title, scTrackId, stage);
    if (exact) return exact;
    const ranked = await this.worker.rankLyrics(
      artist,
      title,
      candidates.map((c, idx) => ({ idx, source: c.source, snippet: this.buildSnippet(c) })),
    );
    this.logger.log(`${stage} ranked ${scTrackId}: ${JSON.stringify(ranked)}`);
    if (ranked && ranked.score >= MIN_RANK_SCORE) {
      const pick = candidates[ranked.best_idx];
      if (pick) {
        this.logger.log(
          `${stage} picked source=${pick.source} score=${ranked.score} ` +
            `(${pick.artistGuess ?? '?'} - ${pick.titleGuess ?? '?'}) for ${scTrackId}`,
        );
        return pick;
      }
    }
    this.logger.log(
      `${stage} score ${ranked?.score ?? '?'} below threshold ${MIN_RANK_SCORE} for ${scTrackId}`,
    );
    return null;
  }

  private pickExactMatch(
    candidates: Candidate[],
    queries: string[],
    artist: string,
    title: string,
    scTrackId: string,
    stage: string,
  ): Candidate | null {
    const a = canonMeta(artist);
    const t = canonMeta(title);
    const querySet = new Set<string>();
    for (const q of queries) {
      const c = canonMeta(q);
      if (c) querySet.add(c);
    }
    for (const c of candidates) {
      const ca = canonMeta(c.artistGuess ?? '');
      const ct = canonMeta(c.titleGuess ?? '');
      if (!ca || !ct) continue;
      if (a && t && ca === a && ct === t) {
        this.logger.log(
          `${stage} exact match (direct) for ${scTrackId}: ${c.source} ` +
            `"${c.artistGuess} - ${c.titleGuess}", skipping AI rank`,
        );
        return c;
      }
      const fwd = `${ca} ${ct}`;
      const rev = `${ct} ${ca}`;
      if (querySet.has(fwd) || querySet.has(rev)) {
        this.logger.log(
          `${stage} exact match (via query) for ${scTrackId}: ${c.source} ` +
            `"${c.artistGuess} - ${c.titleGuess}", skipping AI rank`,
        );
        return c;
      }
    }
    return null;
  }

  private filterByMetadata(
    candidates: Candidate[],
    artist: string,
    title: string,
    durationSec: number | undefined,
    scTrackId: string,
  ): Candidate[] {
    const source = `${artist} ${title}`.trim();
    const out: Candidate[] = [];
    for (const c of candidates) {
      const candMeta = `${c.artistGuess ?? ''} ${c.titleGuess ?? ''}`.trim();
      if (candMeta) {
        const overlap = metaOverlap(source, candMeta);
        if (overlap < MIN_META_OVERLAP) {
          this.logger.debug(
            `drop ${c.source} "${c.artistGuess}-${c.titleGuess}" for ${scTrackId}: ` +
              `meta overlap ${overlap.toFixed(2)} < ${MIN_META_OVERLAP}`,
          );
          continue;
        }
      }
      if (durationSec && c.durationSec) {
        const diff = Math.abs(durationSec - c.durationSec) / Math.max(durationSec, c.durationSec);
        if (diff > MAX_DURATION_DIFF) {
          this.logger.debug(
            `drop ${c.source} "${c.artistGuess}-${c.titleGuess}" for ${scTrackId}: ` +
              `duration ${c.durationSec}s vs ${durationSec}s (diff ${(diff * 100).toFixed(0)}%)`,
          );
          continue;
        }
      }
      out.push(c);
    }
    this.logger.log(
      `metadata filter for ${scTrackId}: ${out.length}/${candidates.length} candidates survived`,
    );
    return out;
  }

  private async fanoutSearch(queries: string[]): Promise<Candidate[]> {
    const uniqueQueries = [...new Set(queries.map((q) => q.trim()).filter(Boolean))].slice(0, 4);
    const tasks: Array<Promise<Candidate[]>> = [];

    for (const q of uniqueQueries) {
      tasks.push(
        this.lrclib.searchByQuery(q, 10).then((rs) =>
          rs.map<Candidate>((r) => ({
            source: 'lrclib',
            syncedLrc: r.syncedLrc,
            plainText: r.plainText ?? (r.syncedLrc ? stripLrcTimestamps(r.syncedLrc) : null),
            artistGuess: r.artistGuess,
            titleGuess: r.titleGuess,
            durationSec: r.durationSec,
          })),
        ),
      );
      tasks.push(
        this.mxm.searchByQuery(q, 10).then((rs) =>
          rs.map<Candidate>((r) => ({
            source: 'musixmatch',
            syncedLrc: r.syncedLrc,
            plainText: r.plainText ?? (r.syncedLrc ? stripLrcTimestamps(r.syncedLrc) : null),
            artistGuess: r.artistGuess,
            titleGuess: r.titleGuess,
            durationSec: r.durationSec,
          })),
        ),
      );
      tasks.push(
        this.genius.searchByQuery(q, 10).then((rs) =>
          rs.map<Candidate>((r) => ({
            source: 'genius',
            syncedLrc: null,
            plainText: r.plainText,
            artistGuess: r.artistGuess,
            titleGuess: r.titleGuess,
          })),
        ),
      );
      tasks.push(
        this.netease.searchByQuery(q, 5).then((rs) =>
          rs.map<Candidate>((r) => ({
            source: 'netease',
            syncedLrc: r.syncedLrc,
            plainText: r.plainText ?? (r.syncedLrc ? stripLrcTimestamps(r.syncedLrc) : null),
            artistGuess: r.artistGuess,
            titleGuess: r.titleGuess,
            durationSec: r.durationSec,
          })),
        ),
      );
    }

    const arrays = await Promise.all(tasks);
    const all = arrays.flat();
    return this.dedupe(all).slice(0, MAX_CANDIDATES);
  }

  async handleUploaded(scTrackIdRaw: string, storageUrl: string): Promise<void> {
    const scTrackId = normalizeScTrackId(scTrackIdRaw);
    if (!scTrackId || !storageUrl) return;

    const existing = this.whisperInflight.get(scTrackId);
    if (existing) return existing;

    const promise = (async () => {
      try {
        const ext = this.inflight.get(scTrackId);
        if (ext) await ext.catch(() => {});

        const entity = await this.db.query.lyricsCache.findFirst({
          where: eq(lyricsCache.scTrackId, scTrackId),
        });
        if (entity?.syncedLrc) return;

        if (entity?.plainText) {
          await this.alignWithWhisper(entity, storageUrl);
          return;
        }

        await this.fullTranscribe(scTrackId, storageUrl);
      } finally {
        this.whisperInflight.delete(scTrackId);
      }
    })();
    this.whisperInflight.set(scTrackId, promise);
    return promise;
  }

  private async alignWithWhisper(entity: LyricsCache, storageUrl: string): Promise<void> {
    if (!entity.plainText || entity.syncedLrc) return;
    let result: Awaited<ReturnType<WorkerClient['transcribeAudio']>>;
    try {
      result = await this.worker.transcribeAudio(
        storageUrl,
        entity.language ?? undefined,
        entity.plainText.slice(0, 2000),
      );
    } catch (e) {
      this.logger.warn(`align ${entity.scTrackId}: transcribe failed: ${(e as Error).message}`);
      return;
    }
    if (!result?.syncedLrc) return;
    await this.db
      .update(lyricsCache)
      .set({ syncedLrc: result.syncedLrc })
      .where(eq(lyricsCache.scTrackId, entity.scTrackId));
    this.logger.log(`aligned sync LRC for ${entity.scTrackId}`);
  }

  private async reapWhisper(): Promise<void> {
    const cutoff = new Date(Date.now() - REAP_MIN_AGE_MS);

    const needAlign = await this.db
      .select({ scTrackId: lyricsCache.scTrackId })
      .from(lyricsCache)
      .where(
        and(
          isNotNull(lyricsCache.plainText),
          sql`length(${lyricsCache.plainText}) > 0`,
          isNull(lyricsCache.syncedLrc),
          lt(lyricsCache.createdAt, cutoff),
        ),
      )
      .orderBy(asc(lyricsCache.createdAt))
      .limit(REAP_LIMIT_ALIGN);

    const needFull = await this.db
      .select({ scTrackId: indexedTracks.scTrackId })
      .from(indexedTracks)
      .leftJoin(lyricsCache, eq(lyricsCache.scTrackId, indexedTracks.scTrackId))
      .where(
        and(
          isNotNull(indexedTracks.indexedAt),
          isNull(lyricsCache.scTrackId),
          lt(indexedTracks.createdAt, cutoff),
        ),
      )
      .orderBy(asc(indexedTracks.createdAt))
      .limit(REAP_LIMIT_FULL);

    const ids = [...needAlign.map((r) => r.scTrackId), ...needFull.map((r) => r.scTrackId)];
    if (!ids.length) return;
    this.logger.log(
      `[lyrics-reap] retrying whisper for ${needAlign.length} align + ${needFull.length} full`,
    );
    for (const id of ids) {
      this.trigger.trigger(id);
    }
  }

  private async reapEmbeds(): Promise<void> {
    const cutoff = new Date(Date.now() - REAP_MIN_AGE_MS);
    const stuck = await this.db
      .select()
      .from(lyricsCache)
      .where(
        and(
          isNull(lyricsCache.embeddedAt),
          lt(lyricsCache.createdAt, cutoff),
          sql`length(coalesce(${lyricsCache.plainText}, ${lyricsCache.syncedLrc}, '')) > 30`,
        ),
      )
      .orderBy(asc(lyricsCache.createdAt))
      .limit(REAP_LIMIT_FULL);
    if (!stuck.length) return;

    this.logger.log(`[lyrics-reap] re-publishing embed for ${stuck.length} stuck rows`);
    for (const entity of stuck) {
      const text = pickLyricsText(entity.plainText, entity.syncedLrc);
      if (!text || text.length <= 30) continue;
      this.afterFound(entity, text).catch((e) =>
        this.logger.warn(`embed-reap ${entity.scTrackId}: ${(e as Error).message}`),
      );
    }
  }

  private async fullTranscribe(scTrackId: string, storageUrl: string): Promise<void> {
    let result: Awaited<ReturnType<WorkerClient['transcribeAudio']>>;
    try {
      result = await this.worker.transcribeAudio(storageUrl);
    } catch (e) {
      this.logger.warn(`self-gen ${scTrackId}: transcribe failed: ${(e as Error).message}`);
      return;
    }
    if (!result || (!result.syncedLrc && !result.plainText)) {
      this.logger.log(`self-gen ${scTrackId}: whisper returned empty`);
      return;
    }

    const [entity] = await this.db
      .insert(lyricsCache)
      .values({
        scTrackId,
        syncedLrc: result.syncedLrc,
        plainText: result.plainText,
        source: 'self_gen',
        language: null,
        languageConfidence: null,
        embeddedAt: null,
      })
      .returning();
    this.logger.log(`self-generated LRC for ${scTrackId} (lang=${result.language})`);

    const text = pickLyricsText(result.plainText, result.syncedLrc);
    if (text && text.length > 30) {
      this.afterFound(entity, text).catch((e) =>
        this.logger.warn(`after-found ${scTrackId}: ${(e as Error).message}`),
      );
    }
  }

  private buildSnippet(c: Candidate): string {
    const text = c.plainText ?? (c.syncedLrc ? stripLrcTimestamps(c.syncedLrc) : '');
    const guess =
      c.artistGuess || c.titleGuess ? `(${c.artistGuess ?? '?'} — ${c.titleGuess ?? '?'}) ` : '';
    return (guess + text).slice(0, SNIPPET_LEN);
  }

  private dedupe(candidates: Candidate[]): Candidate[] {
    const seen = new Set<string>();
    const out: Candidate[] = [];
    for (const c of candidates) {
      const body = (c.plainText ?? c.syncedLrc ?? '').slice(0, 80).replace(/\s+/g, ' ').trim();
      if (!body) continue;
      const key = body.toLowerCase();
      if (seen.has(key)) continue;
      seen.add(key);
      out.push(c);
    }
    return out;
  }

  private async afterFound(entity: LyricsCache, text: string): Promise<void> {
    let lang: { language: string; confidence: number } | null = null;
    try {
      lang = await this.worker.detectLanguage(text.slice(0, 2000));
    } catch (e) {
      this.logger.debug(
        `detectLanguage worker error for ${entity.scTrackId}: ${(e as Error).message}`,
      );
    }
    if (!lang) {
      lang = detectLanguageHeuristic(text);
      if (lang) {
        this.logger.log(
          `detectLanguage ${entity.scTrackId}: worker returned null, heuristic → ${lang.language} (${lang.confidence.toFixed(2)})`,
        );
      } else {
        this.logger.warn(
          `detectLanguage ${entity.scTrackId}: both worker and heuristic returned null`,
        );
      }
    } else {
      this.logger.log(
        `detectLanguage ${entity.scTrackId}: worker → ${lang.language} (${lang.confidence.toFixed(2)})`,
      );
    }
    if (lang) {
      entity.language = lang.language;
      entity.languageConfidence = lang.confidence;
      await this.db
        .update(lyricsCache)
        .set({ language: lang.language, languageConfidence: lang.confidence })
        .where(eq(lyricsCache.scTrackId, entity.scTrackId));
      await this.db
        .update(indexedTracks)
        .set({ language: lang.language, languageConfidence: lang.confidence })
        .where(eq(indexedTracks.scTrackId, entity.scTrackId));
    }

    await this.nats.publish(SUBJECTS.embedLyrics, {
      sc_track_id: entity.scTrackId,
      text: text.slice(0, 4000),
      language: entity.language,
    });
  }

  private toResponse(entity: LyricsCache): LyricsResponse {
    return {
      scTrackId: entity.scTrackId,
      syncedLrc: entity.syncedLrc,
      plainText: entity.plainText,
      source: entity.source,
      language: entity.language,
      languageConfidence: entity.languageConfidence,
    };
  }
}
