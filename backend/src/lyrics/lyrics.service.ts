import { Injectable, Logger, OnModuleDestroy, OnModuleInit } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { IsNull, Repository } from 'typeorm';
import { NatsService } from '../bus/nats.service.js';
import { STREAMS, SUBJECTS } from '../bus/subjects.js';
import { IndexedTrack } from '../indexing/entities/indexed-track.entity.js';
import { TranscodeTriggerService } from '../transcode/transcode-trigger.service.js';
import { LyricsCache, LyricsSource } from './entities/lyrics-cache.entity.js';
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
import { WorkerClient } from './worker.client.js';

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

/** Cap concurrent lyrics lookups kicked off by indexing so the worker queues don't explode. */
const INDEXING_CONCURRENCY = Number.parseInt(process.env.LYRICS_INDEXING_CONCURRENCY ?? '3', 10);

/** Whisper-reap: периодически добиваем треки, у которых alignment/full-transcribe
 *  не сработал с первого storage-event'а (whisper упал, NATS лёг и т.п.). */
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
  /** External-lookup промисы (lrclib/mxm/genius). handleUploaded ждёт их перед whisper'ом. */
  private readonly inflight = new Map<string, Promise<LyricsResponse>>();
  /** Whisper-промисы (align/full). Дедуп повторных storage-events для одного трека. */
  private readonly whisperInflight = new Map<string, Promise<void>>();
  private readonly indexingSem = new Semaphore(INDEXING_CONCURRENCY);
  private reapTimer?: NodeJS.Timeout;

  constructor(
    @InjectRepository(LyricsCache)
    private readonly cache: Repository<LyricsCache>,
    @InjectRepository(IndexedTrack)
    private readonly indexed: Repository<IndexedTrack>,
    private readonly nats: NatsService,
    private readonly lrclib: LrclibService,
    private readonly mxm: MusixmatchService,
    private readonly genius: GeniusService,
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
        await this.cache.update(
          { scTrackId: payload.sc_track_id, embeddedAt: IsNull() },
          { embeddedAt: () => 'now()' as any },
        );
      },
      SUBJECTS.doneEmbedLyrics,
    );
    this.reapTimer = setInterval(() => this.reapWhisper().catch(() => {}), REAP_INTERVAL_MS);
  }

  onModuleDestroy(): void {
    if (this.reapTimer) clearInterval(this.reapTimer);
  }

  /**
   * Endpoint 1 — поиск по URN/id. Backend сам тянет artist/title/duration из
   * indexed_tracks.rawScData, читает/пишет кеш. Используется при воспроизведении
   * трека и индексации из axios interceptor.
   */
  async ensureLyrics(scTrackIdRaw: string): Promise<LyricsResponse> {
    const scTrackId = normalizeScTrackId(scTrackIdRaw) ?? scTrackIdRaw;
    const cached = await this.cache.findOne({ where: { scTrackId } });
    if (cached) return this.toResponse(cached);

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

  /**
   * Endpoint 2 — ручной поиск по artist/title (preview).
   *
   * Кеш НЕ трогаем (ни чтение, ни запись), ни PG ни qdrant. Юзер вводит
   * произвольные данные — нельзя attach'ить результат к scTrackId, иначе:
   *   - если ввели чужой "artist - title" и внешние нашли совпадение → лирика
   *     чужого трека сохранится на урн текущего;
   *   - stage3 self-gen транскрибит реальный аудио по урну (игнорит ввод),
   *     произвольный поиск триггерит whisper на чужой трек.
   *
   * Если будем делать "сохранить найденное на этот трек" — это ОТДЕЛЬНОЕ
   * действие пользователя (кнопка "Apply"), не побочка search-запроса.
   */
  async searchLyrics(hints: LyricsHints): Promise<LyricsResponse> {
    if (!hints.title || !hints.artist) return this.emptyResponse(null);
    return this.runPipeline(null, hints as Required<LyricsHints>, false);
  }

  /**
   * Бизнес-обработчик для IndexingService: тот же ensureLyrics, только
   * под семафором — массовая индексация не должна забивать очередь воркера.
   */
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

    const entity = this.cache.create({
      scTrackId,
      syncedLrc: picked.syncedLrc,
      plainText: picked.plainText,
      source: picked.source,
      language: null,
      languageConfidence: null,
      embeddedAt: null,
    });
    await this.cache.save(entity);

    const textForLang = pickLyricsText(picked.plainText, picked.syncedLrc);
    if (textForLang && textForLang.length > 30) {
      this.afterFound(entity, textForLang).catch((e) =>
        this.logger.warn(`after-found ${scTrackId}: ${(e as Error).message}`),
      );
    }
    // alignment plainText→syncedLrc делает handleUploaded, когда трек реально в storage.

    return this.toResponse(entity);
  }

  private async loadHintsFromDb(scTrackId: string): Promise<LyricsHints> {
    const row = await this.indexed.findOne({
      where: { scTrackId },
      select: ['title', 'durationMs', 'rawScData'],
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

    // stage3 (whisper self-gen) уехал в handleUploaded — он крутится только когда
    // трек реально доступен в storage, а внешний поиск ничего не дал.
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
      // 1) Прямое совпадение с исходными artist/title.
      if (a && t && ca === a && ct === t) {
        this.logger.log(
          `${stage} exact match (direct) for ${scTrackId}: ${c.source} ` +
            `"${c.artistGuess} - ${c.titleGuess}", skipping AI rank`,
        );
        return c;
      }
      // 2) Совпадение с любым heuristic query (обе перестановки artist/title).
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
    }

    const arrays = await Promise.all(tasks);
    const all = arrays.flat();
    return this.dedupe(all).slice(0, MAX_CANDIDATES);
  }

  /**
   * Реактивный обработчик `storage.track_uploaded` — крутит whisper, когда трек
   * реально лежит в backend. Маршруты:
   *   - есть syncedLrc       → ничего не делаем;
   *   - есть только plainText → align (whisper с initial_prompt=plainText);
   *   - записи нет           → full transcribe (внешние сервисы ничего не нашли).
   * Перед whisper'ом ждёт inflight внешнего поиска, чтобы не запустить виспер
   * параллельно с lrclib/mxm/genius. Дедуп через whisperInflight — повторные
   * events на тот же трек не плодят дублей.
   */
  async handleUploaded(scTrackIdRaw: string, storageUrl: string): Promise<void> {
    const scTrackId = normalizeScTrackId(scTrackIdRaw);
    if (!scTrackId || !storageUrl) return;

    const existing = this.whisperInflight.get(scTrackId);
    if (existing) return existing;

    const promise = (async () => {
      try {
        // дождаться внешнего поиска, если он ещё крутится
        const ext = this.inflight.get(scTrackId);
        if (ext) {
          await ext.catch(() => {});
        }

        const entity = await this.cache.findOne({ where: { scTrackId } });
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

  /** plain → synced via whisper с initial_prompt. */
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
    await this.cache.update({ scTrackId: entity.scTrackId }, { syncedLrc: result.syncedLrc });
    this.logger.log(`aligned sync LRC for ${entity.scTrackId}`);
  }

  /**
   * Периодический проход для треков, у которых whisper не отработал с первого
   * storage-event'а (упал воркер, потерялся ack, и т.п.). Дёргает trigger —
   * стриминг сделает HEAD storage и для cached треков publish'нёт событие
   * заново; handleUploaded подхватит и попробует ещё раз. Не сохраняем null,
   * так что повторные сухие проходы не превращаются в false-negative.
   */
  private async reapWhisper(): Promise<void> {
    const cutoff = new Date(Date.now() - REAP_MIN_AGE_MS);

    // Treck #1: есть plainText, но нет syncedLrc — нужен alignment.
    const needAlign = await this.cache
      .createQueryBuilder('l')
      .select('l.scTrackId', 'scTrackId')
      .where('l.plainText IS NOT NULL')
      .andWhere('length(l.plainText) > 0')
      .andWhere('l.syncedLrc IS NULL')
      .andWhere('l.createdAt < :cutoff', { cutoff })
      .orderBy('l.createdAt', 'ASC')
      .limit(REAP_LIMIT_ALIGN)
      .getRawMany<{ scTrackId: string }>();

    // Treck #2: индекс есть (трек уехал в storage), а в lyrics_cache записи нет
    // (внешний поиск пуст; с того момента whisper мог упасть). full-transcribe.
    const needFull = await this.indexed
      .createQueryBuilder('t')
      .select('t.scTrackId', 'scTrackId')
      .leftJoin(LyricsCache, 'l', 'l.sc_track_id = t.sc_track_id')
      .where('t.indexedAt IS NOT NULL')
      .andWhere('l.sc_track_id IS NULL')
      .andWhere('t.createdAt < :cutoff', { cutoff })
      .orderBy('t.createdAt', 'ASC')
      .limit(REAP_LIMIT_FULL)
      .getRawMany<{ scTrackId: string }>();

    const ids = [...needAlign.map((r) => r.scTrackId), ...needFull.map((r) => r.scTrackId)];
    if (!ids.length) return;
    this.logger.log(
      `[lyrics-reap] retrying whisper for ${needAlign.length} align + ${needFull.length} full`,
    );
    for (const id of ids) {
      this.trigger.trigger(id);
    }
  }

  /** Внешний поиск пуст → виспер с нуля. На пустой результат — ничего не сохраняем. */
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

    const entity = this.cache.create({
      scTrackId,
      syncedLrc: result.syncedLrc,
      plainText: result.plainText,
      source: 'self_gen',
      language: null,
      languageConfidence: null,
      embeddedAt: null,
    });
    await this.cache.save(entity);
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

  /**
   * `text` — лучший доступный текст лирики (plain или stripped synced),
   * заранее выбранный через pickLyricsText. Используется и для детекта языка,
   * и для embed в qdrant.
   */
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
      await this.cache.update(
        { scTrackId: entity.scTrackId },
        { language: lang.language, languageConfidence: lang.confidence },
      );
      await this.indexed.update(
        { scTrackId: entity.scTrackId },
        { language: lang.language, languageConfidence: lang.confidence },
      );
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
