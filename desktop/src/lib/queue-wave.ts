/**
 * The recommendation «wave»: the continuation of the queue once the playback context
 * (playlist/likes/…) has played to the end — like autoplay in the official SC client.
 *
 * The chain: related of the current seed track (pages in sequence), then, if related
 * is exhausted, shuffled tracks by the same artist (a local «station», the same
 * trick as the station cards in Search.tsx). The autopilot calls fetchWaveTracks
 * with the last track in the queue, so the seed is new every time and the chain keeps
 * going while fresh tracks turn up.
 *
 * State lives for one playback session: resetWaveState() is called by the
 * autopilot on a new play() (see lib/queue-autopilot.ts).
 */

import { shuffleArray, type Track, usePlayerStore } from '../stores/player';
import { api } from './api';
import type { PagedResponse } from './hooks';
import { fetchRelatedTracks } from './related';

/** Related page size: same as SC itself */
const RELATED_PAGE_SIZE = 20;
/** How many related pages to walk per call before falling back to the artist */
const RELATED_MAX_PAGES = 3;
/** Artist fallback: 2 pages of 50 — enough for a local «station» */
const ARTIST_PAGE_SIZE = 50;
const ARTIST_MAX_PAGES = 2;
/** Never push more than this into the queue per call */
const WAVE_BATCH_MAX = 20;

// Seed of the related chain and its page cursor.
let seedUrn: string | null = null;
let seedPage = 0;
// Artist-fallback pool: one artist at a time, shuffled.
let artistId: string | null = null;
let artistPool: Track[] = [];
// Artists whose tracks all went into the queue already (not pulled twice).
const exhaustedArtists = new Set<string>();

export function resetWaveState(): void {
  seedUrn = null;
  seedPage = 0;
  artistId = null;
  artistPool = [];
  exhaustedArtists.clear();
}

/** Take fresh tracks (not queued yet) from a batch, at most `max` */
function takeFresh(batch: Track[], existing: Set<string>, max: number): Track[] {
  const out: Track[] = [];
  for (const track of batch) {
    if (out.length >= max) break;
    if (!track?.urn || existing.has(track.urn)) continue;
    existing.add(track.urn);
    out.push(track);
  }
  return out;
}

/**
 * Related chain: the seed's pages in sequence. Pages accumulate within one call
 * until WAVE_BATCH_MAX fresh tracks are collected or the seed's related runs out.
 */
async function fromRelated(seed: Track, existing: Set<string>): Promise<Track[]> {
  if (seed.urn !== seedUrn) {
    seedUrn = seed.urn;
    seedPage = 0;
  }
  const out: Track[] = [];
  for (let i = 0; i < RELATED_MAX_PAGES && out.length < WAVE_BATCH_MAX; i++) {
    const res = await fetchRelatedTracks(seed.urn, RELATED_PAGE_SIZE, seedPage);
    seedPage += 1;
    out.push(...takeFresh(res.collection, existing, WAVE_BATCH_MAX - out.length));
    if (!res.has_more) break;
  }
  return out;
}

/** All of the artist's tracks (1–2 pages), shuffled. */
async function fetchArtistPool(id: string): Promise<Track[]> {
  const all: Track[] = [];
  for (let page = 0; page < ARTIST_MAX_PAGES; page++) {
    const res = await api<PagedResponse<Track>>(
      `/users/${encodeURIComponent(id)}/tracks?limit=${ARTIST_PAGE_SIZE}&page=${page}`,
    );
    all.push(...(res.collection ?? []));
    if (!res.has_more) break;
  }
  shuffleArray(all);
  return all;
}

/** Fallback: the seed track's artist's tracks, as the pool drains. */
async function fromArtist(seed: Track, existing: Set<string>): Promise<Track[]> {
  const id = seed.user?.id != null ? String(seed.user.id) : null;
  if (!id || exhaustedArtists.has(id)) return [];
  if (id !== artistId) {
    artistId = id;
    artistPool = [];
  }
  if (artistPool.length === 0) {
    artistPool = await fetchArtistPool(id);
    if (artistPool.length === 0) {
      exhaustedArtists.add(id);
      return [];
    }
  }
  const out = takeFresh(artistPool, existing, WAVE_BATCH_MAX);
  if (out.length === 0) exhaustedArtists.add(id);
  return out;
}

/**
 * The next batch of the «wave». An empty array = no more recommendations —
 * the autopilot pauses.
 */
export async function fetchWaveTracks(seed: Track): Promise<Track[]> {
  const existing = new Set(usePlayerStore.getState().queue.map((t) => t.urn));
  try {
    const related = await fromRelated(seed, existing);
    if (related.length > 0) return related;
  } catch (e) {
    console.debug('[wave] related failed:', e);
  }
  try {
    return await fromArtist(seed, existing);
  } catch (e) {
    console.debug('[wave] artist fallback failed:', e);
    return [];
  }
}
