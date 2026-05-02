import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { firstValueFrom } from 'rxjs';

const NCM_API = process.env.NETEASE_API_BASE ?? 'https://ncm.nekohasegawa.com';
const TIMEOUT_MS = 12000;
const SEARCH_LIMIT = 5;

export interface NeteaseResult {
  syncedLrc: string | null;
  plainText: string | null;
  artistGuess?: string;
  titleGuess?: string;
  durationSec?: number;
}

interface NcmArtist {
  name?: string;
}

interface NcmSong {
  id?: number;
  name?: string;
  duration?: number;
  dt?: number;
  artists?: NcmArtist[];
  ar?: NcmArtist[];
}

interface NcmSearchResp {
  result?: { songs?: NcmSong[] };
}

interface NcmLyricResp {
  lrc?: { lyric?: string };
  tlyric?: { lyric?: string };
}

/**
 * NetEase Cloud Music lyrics provider — strong source of synchronized LRC for
 * Chinese / Korean / Japanese / Russian / Vietnamese tracks that LRCLIB / MXM
 * often miss.
 */
@Injectable()
export class NeteaseService {
  private readonly logger = new Logger(NeteaseService.name);

  constructor(private readonly http: HttpService) {}

  async searchByQuery(q: string, limit = SEARCH_LIMIT): Promise<NeteaseResult[]> {
    const songs = await this.search(q, limit);
    if (!songs.length) return [];
    const lyrics = await Promise.all(songs.map((s) => this.fetchLrc(s.id ?? 0)));
    const out: NeteaseResult[] = [];
    for (let i = 0; i < songs.length; i++) {
      const lrc = lyrics[i];
      if (!lrc) continue;
      const song = songs[i];
      const artistGuess = (song.artists || song.ar || [])
        .map((a) => a?.name || '')
        .filter(Boolean)
        .join(', ');
      const ms = song.duration ?? song.dt;
      out.push({
        syncedLrc: lrc.synced,
        plainText: lrc.plain,
        artistGuess: artistGuess || undefined,
        titleGuess: song.name,
        durationSec: typeof ms === 'number' && ms > 0 ? Math.round(ms / 1000) : undefined,
      });
    }
    return out;
  }

  private async search(q: string, limit: number): Promise<NcmSong[]> {
    try {
      const url = `${NCM_API}/search?keywords=${encodeURIComponent(q)}&type=1&limit=${limit}`;
      const resp = await firstValueFrom(this.http.get<NcmSearchResp>(url, { timeout: TIMEOUT_MS }));
      const list = resp.data?.result?.songs ?? [];
      return list
        .filter((s): s is NcmSong & { id: number } => typeof s.id === 'number')
        .slice(0, limit);
    } catch (e) {
      this.logger.debug(`netease search failed: ${(e as Error).message}`);
      return [];
    }
  }

  private async fetchLrc(
    id: number,
  ): Promise<{ synced: string | null; plain: string | null } | null> {
    if (!id) return null;
    try {
      const url = `${NCM_API}/lyric?id=${id}`;
      const resp = await firstValueFrom(this.http.get<NcmLyricResp>(url, { timeout: TIMEOUT_MS }));
      const synced = (resp.data?.lrc?.lyric ?? '').trim() || null;
      // tlyric (translation) is fallback only for plain text — main LRC is always primary
      const plainSrc = synced || (resp.data?.tlyric?.lyric ?? '').trim();
      const plain = plainSrc ? this.stripLrcTimestamps(plainSrc) : null;
      if (!synced && !plain) return null;
      return { synced, plain };
    } catch (e) {
      this.logger.debug(`netease lyric ${id} failed: ${(e as Error).message}`);
      return null;
    }
  }

  private stripLrcTimestamps(lrc: string): string {
    return lrc
      .split('\n')
      .map((line) => line.replace(/\[[^\]\r\n]*\]\s*/g, '').trim())
      .filter(Boolean)
      .join('\n');
  }
}
