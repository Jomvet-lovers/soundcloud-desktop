const LYRICS_API = 'https://lrclib.net/api';
const TIMEOUT_MS = 10000;

interface LrcLibResponse {
  plainLyrics?: string;
  syncedLyrics?: string;
}

export async function searchLyrics(artist: string, title: string): Promise<string | null> {
  try {
    const cleanArtist = artist.replace(/[^\w\s]/g, '').trim();
    const cleanTitle = title.replace(/[^\w\s]/g, '').trim();
    
    if (!cleanArtist || !cleanTitle) return null;
    
    const params = new URLSearchParams({
      artist_name: cleanArtist,
      track_name: cleanTitle,
    });
    const url = `${LYRICS_API}/search?${params}`;
    
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), TIMEOUT_MS);
    
    const res = await fetch(url, { signal: controller.signal });
    clearTimeout(timeoutId);
    
    if (!res.ok) return null;
    
    const data: LrcLibResponse[] = await res.json();
    if (!data || data.length === 0) return null;
    
    return data[0].plainLyrics || data[0].syncedLyrics || null;
  } catch (e) {
    console.error('Lyrics fetch failed:', e);
    return null;
  }
}
