import { useQuery } from '@tanstack/react-query';
import type { Track } from '../stores/player';
import { api } from './api';

export interface RecommendResult {
  id: string | number;
  score?: number;
  payload?: Record<string, unknown>;
}

export interface IndexingStats {
  indexed: number;
  pending: number;
}

const SW_STALE_MS = 0;
const SW_GC_MS = 1000 * 60 * 5;

function normLanguages(langs: string[] | undefined): string | undefined {
  if (!langs || langs.length === 0) return undefined;
  return [...langs].sort().join(',');
}

/**
 * Hydrate Qdrant numeric IDs → full SC track metadata, preserving recommendation order.
 *
 * Per-track `/tracks/:urn` returns full metadata with real duration (vs. the
 * preview-only public search endpoint). Backend caches these for 10m so a warm
 * cache is effectively free; a cold cache fans out the requests in parallel.
 */
export async function hydrateByIds(recs: RecommendResult[]): Promise<Track[]> {
  const urns = recs
    .map((r) => {
      const id = String(r.id);
      return id ? `soundcloud:tracks:${id}` : null;
    })
    .filter((u): u is string => u !== null);
  if (!urns.length) return [];

  const results = await Promise.all(
    urns.map((urn) =>
      api<Track>(`/tracks/${encodeURIComponent(urn)}`).catch(() => null as Track | null),
    ),
  );

  return results.filter((t): t is Track => t !== null);
}

/**
 * Free-form vibe search. Returns hydrated tracks in Qdrant score order.
 * Kept flat (not cluster-grouped) — search is a single-intent query.
 */
export function useSoundWaveSearch(opts: { q: string; languages?: string[]; limit?: number }) {
  const q = opts.q.trim();
  const limit = opts.limit ?? 24;
  const languages = normLanguages(opts.languages);

  return useQuery({
    queryKey: ['soundwave', 'search', q, limit, languages ?? 'all'],
    enabled: q.length >= 2,
    staleTime: SW_STALE_MS,
    gcTime: SW_GC_MS,
    queryFn: async () => {
      const qs = new URLSearchParams({ q, limit: String(limit) });
      if (languages) qs.set('languages', languages);

      const recs = await api<RecommendResult[]>(
        `/recommendations/search?${qs}`,
        undefined,
        30_000,
      ).catch(() => [] as RecommendResult[]);
      if (!recs.length) return { tracks: [] as Track[], recs };

      const tracks = await hydrateByIds(recs);
      return { tracks, recs };
    },
  });
}

export type SmartWaveSeedKind = 'user' | 'track' | 'artist';

export interface SmartWaveBatch {
  tracks: Track[];
  cursor: string;
}

interface SmartWavePayload {
  tracks: RecommendResult[];
  cursor: string;
}

function smartWaveUrl(
  seedKind: SmartWaveSeedKind,
  seedId: string | undefined,
  qs: URLSearchParams,
): string {
  switch (seedKind) {
    case 'user':
      return `/recommendations/wave${qs.toString() ? `?${qs}` : ''}`;
    case 'track':
      return `/recommendations/wave/from-track/${encodeURIComponent(seedId!)}${qs.toString() ? `?${qs}` : ''}`;
    case 'artist':
      return `/recommendations/wave/from-artist/${encodeURIComponent(seedId!)}${qs.toString() ? `?${qs}` : ''}`;
  }
}

/**
 * Запрос порции бесконечной волны. Сервер держит state по cursor'у
 * (Redis, TTL 30 мин) — клиент эхает токен и получает свежие треки без
 * повторов. Если cursor отсутствует или Redis грохнули — сервер начнёт
 * новую сессию волны, для UX это незаметно.
 */
export async function fetchSmartWave(opts: {
  seedKind: SmartWaveSeedKind;
  seedId?: string;
  cursor?: string;
  limit?: number;
  languages?: string[];
}): Promise<SmartWaveBatch> {
  const qs = new URLSearchParams();
  qs.set('limit', String(opts.limit ?? 20));
  if (opts.cursor) qs.set('cursor', opts.cursor);
  const languages = normLanguages(opts.languages);
  if (languages) qs.set('languages', languages);

  const payload = await api<SmartWavePayload>(smartWaveUrl(opts.seedKind, opts.seedId, qs)).catch(
    () => ({ tracks: [], cursor: '' }) as SmartWavePayload,
  );

  if (!payload.tracks.length) {
    return { tracks: [], cursor: payload.cursor };
  }
  const tracks = await hydrateByIds(payload.tracks);
  return { tracks, cursor: payload.cursor };
}

/**
 * Сообщить серверу о dis/pos исходах в недавнем окне волны.
 * Cursor обновится на сервере и следующий fetchSmartWave получит выдачу
 * с адаптированными весами arm'ов.
 */
export async function sendWaveFeedback(opts: {
  cursor: string;
  negatives: number;
  positives: number;
}): Promise<string | null> {
  if (!opts.cursor) return null;
  try {
    const res = await api<{ ok: boolean; cursor?: string | null }>(
      '/recommendations/wave/feedback',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(opts),
      },
    );
    return res?.cursor ?? null;
  } catch {
    return null;
  }
}

/**
 * React-Query обёртка для первой порции волны. Дальше работает в паре с
 * `useInfiniteWave`, который сам шлёт `fetchSmartWave({ cursor })`.
 */
export function useSmartWave(opts: {
  seedKind: SmartWaveSeedKind;
  seedId?: string;
  languages?: string[];
  enabled?: boolean;
  limit?: number;
}) {
  const enabled = opts.enabled !== false && (opts.seedKind === 'user' || !!opts.seedId);
  const languages = normLanguages(opts.languages);

  return useQuery<SmartWaveBatch>({
    queryKey: [
      'smartwave',
      opts.seedKind,
      opts.seedId ?? 'self',
      languages ?? 'all',
      opts.limit ?? 20,
    ],
    enabled,
    staleTime: SW_STALE_MS,
    gcTime: SW_GC_MS,
    queryFn: () =>
      fetchSmartWave({
        seedKind: opts.seedKind,
        seedId: opts.seedId,
        languages: opts.languages,
        limit: opts.limit,
      }),
  });
}

/** Optional lightweight poll of indexing stats. Fails silently if endpoint absent. */
export function useIndexingStats() {
  return useQuery({
    queryKey: ['soundwave', 'indexing-stats'],
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    retry: false,
    queryFn: () => api<IndexingStats>('/indexing/stats').catch(() => null as IndexingStats | null),
  });
}
