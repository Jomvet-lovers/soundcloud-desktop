import {useInfiniteQuery, useQuery} from '@tanstack/react-query';
import {useMemo} from 'react';
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

/**
 * Endless home-wave board for the Search landing — the "затягивающая сетка".
 * Infinite, cursor-paged via `fetchSmartWave({ seedKind: 'user' })`; the server
 * personalises it and degrades to popularity for cold/un-indexed users, so it
 * always returns covers to scroll. Flattened + deduped by urn for a clean grid.
 */
export function useWaveBoard(opts?: { enabled?: boolean; languages?: string[] }) {
    const languages = normLanguages(opts?.languages);
    const query = useInfiniteQuery<
        SmartWaveBatch,
        Error,
        SmartWaveBatch[],
        string[],
        string | undefined
    >({
        queryKey: ['wave', 'board', languages ?? 'all'],
        enabled: opts?.enabled !== false,
        staleTime: SW_STALE_MS,
        gcTime: SW_GC_MS,
        initialPageParam: undefined,
        queryFn: ({pageParam}) =>
            fetchSmartWave({
                seedKind: 'user',
                cursor: pageParam,
                limit: 24,
                languages: opts?.languages,
            }),
        // The server keeps returning a (non-empty) cursor even when a user's wave is
        // exhausted and the page is empty — treat an empty page as end-of-feed so we
        // don't loop forever on a cursor that yields nothing.
        getNextPageParam: (last) => (last.cursor && last.tracks.length > 0 ? last.cursor : undefined),
        select: (data) => data.pages,
    });

    const tracks = useMemo(() => {
        const seen = new Set<string>();
        const out: Track[] = [];
        for (const page of query.data ?? []) {
            for (const t of page.tracks) {
                if (!seen.has(t.urn)) {
                    seen.add(t.urn);
                    out.push(t);
                }
            }
        }
        return out;
    }, [query.data]);

    return {
        tracks,
        isLoading: query.isLoading,
        hasNextPage: query.hasNextPage,
        isFetchingNextPage: query.isFetchingNextPage,
        fetchNextPage: query.fetchNextPage,
    };
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
