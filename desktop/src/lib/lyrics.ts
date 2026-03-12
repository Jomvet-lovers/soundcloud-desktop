const LYRICS_API = 'https://lrclib.net/api';
const TIMEOUT_MS = 10000;

export interface LyricLine {
  time: number;
  text: string;
}

export interface LyricsResult {
  plain: string | null;
  synced: LyricLine[] | null;
}

/** Parse LRC format: [mm:ss.xx] text */
function parseLRC(lrc: string): LyricLine[] {
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

function toResult(entry: { plainLyrics?: string; syncedLyrics?: string }): LyricsResult | null {
  const plain = entry.plainLyrics || null;
  const synced = entry.syncedLyrics ? parseLRC(entry.syncedLyrics) : null;
  if (!plain && !synced) return null;
  return { plain, synced };
}

/** Remove feat/remix/brackets/special chars */
function clean(s: string): string {
  return s
    .replace(/\(feat\.?[^)]*\)/gi, '')
    .replace(/\(ft\.?[^)]*\)/gi, '')
    .replace(/\[.*?\]/g, '')
    .replace(/\(.*?(remix|edit|version|mix|cover|live|acoustic|instrumental|original|prod).*?\)/gi, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Strip everything non-alphanumeric (keep unicode letters) */
function alphaOnly(s: string): string {
  return s.replace(/[^\p{L}\p{N}\s]/gu, '').replace(/\s+/g, ' ').trim();
}

/** Parse "Artist - Title" from a combined string */
function splitArtistTitle(raw: string): [string, string] | null {
  // Try various dash separators
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

async function apiFetch(
  params: Record<string, string>,
  signal: AbortSignal,
): Promise<LyricsResult | null> {
  try {
    const url = `${LYRICS_API}/search?${new URLSearchParams(params)}`;
    const res = await fetch(url, { signal });
    if (!res.ok) return null;
    const data = await res.json();
    if (!data?.length) return null;
    return toResult(data[0]);
  } catch {
    return null;
  }
}

export async function searchLyrics(
  scUsername: string,
  scTitle: string,
): Promise<LyricsResult | null> {
  const controller = new AbortController();
  const tid = setTimeout(() => controller.abort(), TIMEOUT_MS);
  const sig = controller.signal;

  try {
    // 1. Parse "Artist - Title" from SC title (most reliable for reposts/random uploaders)
    const parsed = splitArtistTitle(scTitle);
    if (parsed) {
      const [parsedArtist, parsedTitle] = parsed;
      // 1a. Exact parsed
      let r = await apiFetch({ artist_name: parsedArtist, track_name: parsedTitle }, sig);
      if (r) return r;

      // 1b. Cleaned parsed
      const ca = clean(parsedArtist);
      const ct = clean(parsedTitle);
      r = await apiFetch({ artist_name: ca, track_name: ct }, sig);
      if (r) return r;

      // 1c. Alpha-only parsed
      const aa = alphaOnly(ca);
      const at = alphaOnly(ct);
      if (aa !== ca || at !== ct) {
        r = await apiFetch({ artist_name: aa, track_name: at }, sig);
        if (r) return r;
      }
    }

    // 2. Try SC username + title (works when uploader IS the artist)
    const cleanedTitle = clean(scTitle);
    let r = await apiFetch({ artist_name: scUsername, track_name: cleanedTitle }, sig);
    if (r) return r;

    // 2b. SC username + alpha-only title
    const alphaTitle = alphaOnly(cleanedTitle);
    if (alphaTitle !== cleanedTitle) {
      r = await apiFetch({ artist_name: scUsername, track_name: alphaTitle }, sig);
      if (r) return r;
    }

    // 3. Free-text search (q=) — last resort, catches everything
    //    Combine parsed artist+title or just the full SC title
    const query = parsed ? `${parsed[0]} ${parsed[1]}` : `${scUsername} ${cleanedTitle}`;
    r = await apiFetch({ q: alphaOnly(query) }, sig);
    if (r) return r;

    // 4. If title had "Artist - Title", try q= with just the title part
    if (parsed) {
      r = await apiFetch({ q: alphaOnly(parsed[1]) }, sig);
      if (r) return r;
    }

    return null;
  } finally {
    clearTimeout(tid);
  }
}
