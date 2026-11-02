import { useQuery } from '@tanstack/react-query';
import { useMemo } from 'react';
import type { Track, TrackScdMeta } from '../stores/player';
import { api } from './api';

/**
 * Client-side fetch of `_scd_meta` (the track's status badge) for shelves fed from SC sources.
 *
 * `/tracks/:urn/related` (and public-search) arrive SC-shaped without our meta, and
 * it is needed for the A/C/F badge. The only endpoint that serves it is the
 * single `/tracks/:urn` (the backend caches it for ~10m). We fetch it pointwise and cache
 * it in memory keyed by urn, so the same track in different shelves/genres does not hit
 * the backend again. `null` = the track is not in our catalog → no meta applies, the badge
 * stays hidden (the component is null-safe).
 */
const metaCache = new Map<string, TrackScdMeta | null>();

/** `pending` storage/index will still flip (pending→ok, pending→indexed) —
 *  do NOT pin such meta, otherwise the badge freezes on the first status seen for
 *  the whole session. A terminal one (done/error/too_long/`null`=not in the catalog)
 *  we cache — it no longer changes. */
function isTerminal(meta: TrackScdMeta | null): boolean {
  if (!meta) return true;
  return meta.storage_state !== 'pending' && meta.index_state !== 'pending';
}

async function fetchMeta(urn: string): Promise<TrackScdMeta | null> {
  if (metaCache.has(urn)) return metaCache.get(urn) ?? null;
  const full = await api<Track>(`/tracks/${encodeURIComponent(urn)}`).catch(() => null);
  const meta = full?._scd_meta ?? null;
  if (isTerminal(meta)) metaCache.set(urn, meta); // pending → re-fetch on the next visit
  return meta;
}

async function enrich(tracks: Track[]): Promise<Track[]> {
  const urns = [...new Set(tracks.filter((t) => t.urn && !t._scd_meta).map((t) => t.urn))];
  const resolved = new Map<string, TrackScdMeta | null>();
  await Promise.all(urns.map(async (urn) => resolved.set(urn, await fetchMeta(urn))));
  let changed = false;
  const out = tracks.map((t) => {
    if (t._scd_meta || !t.urn) return t;
    const meta = resolved.get(t.urn);
    if (!meta) return t;
    changed = true;
    return { ...t, _scd_meta: meta };
  });
  return changed ? out : tracks;
}

/**
 * Returns the same tracks, but with `_scd_meta` fetched for the badges. Bundles exactly
 * the passed (rendered) list — not the whole pool. Before loading it returns the original
 * tracks, afterwards the enriched ones.
 */
export function useScdMeta(tracks: Track[]): Track[] {
  const key = useMemo(() => tracks.map((t) => t.urn).join(','), [tracks]);
  const needsEnrich = useMemo(() => tracks.some((t) => t.urn && !t._scd_meta), [tracks]);
  const { data } = useQuery({
    queryKey: ['scd-meta-enrich', key],
    queryFn: () => enrich(tracks),
    enabled: needsEnrich,
    staleTime: 10 * 60 * 1000,
    gcTime: 10 * 60 * 1000,
  });
  return data ?? tracks;
}
