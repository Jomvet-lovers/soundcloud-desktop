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

export type SoundWaveMode = 'similar' | 'diverse';

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

/**
 * Continuation tail seeded by the last queued track. Used by the infinite scroll
 * extension of the home wave's deep_cuts cluster.
 */
export async function fetchWaveTailFromSeed(
  seedTrackId: string,
  opts: { languages?: string[]; mode: SoundWaveMode; limit?: number },
): Promise<RecommendResult[]> {
  const qs = new URLSearchParams({
    limit: String(opts.limit ?? 20),
    mode: opts.mode,
  });
  const languages = normLanguages(opts.languages);
  if (languages) qs.set('languages', languages);
  return api<RecommendResult[]>(
    `/recommendations/tail/${encodeURIComponent(seedTrackId)}?${qs}`,
  ).catch(() => [] as RecommendResult[]);
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
