/**
 * «Волна» рекомендаций: продолжение очереди, когда контекст воспроизведения
 * (плейлист/лайки/…) доигран до конца — как автоплей официального клиента SC.
 *
 * Цепочка: related текущего сид-трека (страницы подряд), затем, если related
 * исчерпан, перемешанные треки того же артиста (локальная «станция», тот же
 * приём, что у карточек станций в Search.tsx). Autopilot зовёт fetchWaveTracks
 * с последним треком очереди, поэтому сид каждый раз новый и цепочка тянется,
 * пока находятся свежие треки.
 *
 * Состояние живёт в пределах сессии воспроизведения: resetWaveState() зовёт
 * autopilot при новом play() (см. lib/queue-autopilot.ts).
 */

import { shuffleArray, type Track, usePlayerStore } from '../stores/player';
import { api } from './api';
import type { PagedResponse } from './hooks';
import { fetchRelatedTracks } from './related';

/** Размер страницы related: как у самой SC. */
const RELATED_PAGE_SIZE = 20;
/** Сколько страниц related перебрать за вызов, прежде чем идти к артисту. */
const RELATED_MAX_PAGES = 3;
/** Фоллбек на артиста: 2 страницы по 50 — хватает на локальную «станцию». */
const ARTIST_PAGE_SIZE = 50;
const ARTIST_MAX_PAGES = 2;
/** Больше этого за один вызов в очередь не подкидываем. */
const WAVE_BATCH_MAX = 20;

// Сид related-цепочки и его курсор страниц.
let seedUrn: string | null = null;
let seedPage = 0;
// Пул артиста-фоллбека: один артист за раз, перемешанный.
let artistId: string | null = null;
let artistPool: Track[] = [];
// Артисты, чьи треки уже полностью ушли в очередь (повторно не тянем).
const exhaustedArtists = new Set<string>();

export function resetWaveState(): void {
  seedUrn = null;
  seedPage = 0;
  artistId = null;
  artistPool = [];
  exhaustedArtists.clear();
}

/** Берёт из пачки свежие треки (не из очереди), не больше `max`. */
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
 * Related-цепочка: страницы сида подряд. Страницы копятся в один вызов,
 * пока не наберём WAVE_BATCH_MAX свежих или у сида не кончится related.
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

/** Все треки артиста (1–2 страницы), перемешанные. */
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

/** Фоллбек: треки артиста сид-трека, по мере расхода пула. */
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
 * Следующая порция «волны». Пустой массив = рекомендаций больше нет —
 * autopilot ставит паузу.
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
