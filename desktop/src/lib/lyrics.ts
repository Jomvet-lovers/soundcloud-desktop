import { api } from './api';

export type LyricsSource = 'lrclib' | 'musixmatch' | 'genius' | 'self_gen' | 'none';

export interface LyricLine {
  time: number;
  text: string;
}

export interface LyricsResult {
  plain: string | null;
  synced: LyricLine[] | null;
  source: LyricsSource;
  language: string | null;
}

interface BackendLyricsResponse {
  scTrackId: string;
  syncedLrc: string | null;
  plainText: string | null;
  source: LyricsSource;
  language: string | null;
  languageConfidence: number | null;
}

/** Parse LRC format: [mm:ss.xx] text */
export function parseLRC(lrc: string): LyricLine[] {
  const lines: LyricLine[] = [];
  for (const raw of lrc.split('\n')) {
    const m = raw.match(/^\[(\d{2}):(\d{2})\.(\d{2,3})\]\s*(.*)/);
    if (!m) continue;
    const time = +m[1] * 60 + +m[2] + +m[3].padEnd(3, '0') / 1000;
    const text = m[4].trim();
    if (text) lines.push({ time, text });
  }
  return lines;
}

/** Parse "Artist - Title" from a combined string */
export function splitArtistTitle(raw: string): [string, string] | null {
  for (const sep of [' - ', ' – ', ' — ', ' // ']) {
    const idx = raw.indexOf(sep);
    if (idx > 0) {
      const artist = raw.slice(0, idx).trim();
      const title = raw.slice(idx + sep.length).trim();
      if (artist && title) return [artist, title];
    }
  }
  return null;
}

function toResult(data: BackendLyricsResponse | null): LyricsResult | null {
  if (!data) return null;
  const synced = data.syncedLrc ? parseLRC(data.syncedLrc) : null;
  return {
    plain: data.plainText,
    synced: synced && synced.length > 0 ? synced : null,
    source: data.source,
    language: data.language,
  };
}

/** Load lyrics by track URN/id. Backend resolves artist/title itself and writes to cache. */
export async function getLyricsByTrack(scTrackId: string): Promise<LyricsResult | null> {
  const data = await api<BackendLyricsResponse>(
    `/lyrics/${encodeURIComponent(scTrackId)}`,
    undefined,
    180_000,
  ).catch(() => null);
  return toResult(data);
}

/** Manual search — preview only. Backend does NOT read or write cache. */
export async function searchLyricsManual(
  artist: string,
  title: string,
  durationMs?: number,
): Promise<LyricsResult | null> {
  const params = new URLSearchParams({ artist, title });
  if (durationMs && Number.isFinite(durationMs) && durationMs > 0) {
    params.set('duration', String(Math.round(durationMs)));
  }
  const data = await api<BackendLyricsResponse>(
    `/lyrics/search?${params}`,
    undefined,
    180_000,
  ).catch(() => null);
  return toResult(data);
}
