import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { firstValueFrom } from 'rxjs';

const MXM_BASE = 'https://apic-desktop.musixmatch.com/ws/1.1';
const APP_ID = 'web-desktop-app-v1.0';
const TOKEN_TTL_MS = 9 * 60 * 60 * 1000;
const TIMEOUT_MS = 15000;

interface MxmSubtitle {
  text: string;
  time: { total: number };
}

export interface MxmCandidate {
  syncedLrc: string | null;
  plainText: string | null;
  artistGuess?: string;
  titleGuess?: string;
  durationSec?: number;
}

interface TrackSearchItem {
  track?: {
    track_id?: number;
    track_name?: string;
    artist_name?: string;
    track_length?: number;
    has_lyrics?: number;
    has_subtitles?: number;
  };
}

@Injectable()
export class MusixmatchService {
  private readonly logger = new Logger(MusixmatchService.name);
  private tokenCache: { token: string; expiresAt: number } | null = null;

  constructor(private readonly http: HttpService) {}

  async searchByQuery(q: string, limit = 3): Promise<MxmCandidate[]> {
    const token = await this.getToken();
    if (!token) return [];

    const tracks = await this.trackSearch(q, token, limit);
    const results: MxmCandidate[] = [];
    for (const t of tracks) {
      const [synced, plain] = await Promise.all([
        t.has_subtitles ? this.subtitleByTrackId(t.track_id, token) : Promise.resolve(null),
        t.has_lyrics ? this.lyricsByTrackId(t.track_id, token) : Promise.resolve(null),
      ]);
      if (!synced && !plain) continue;
      results.push({
        syncedLrc: synced,
        plainText: plain,
        artistGuess: t.artist_name,
        titleGuess: t.track_name,
        durationSec: typeof t.track_length === 'number' ? t.track_length : undefined,
      });
    }
    return results;
  }

  private async trackSearch(
    q: string,
    token: string,
    limit: number,
  ): Promise<
    Array<{
      track_id: number;
      track_name?: string;
      artist_name?: string;
      track_length?: number;
      has_lyrics: number;
      has_subtitles: number;
    }>
  > {
    try {
      const params = new URLSearchParams({
        app_id: APP_ID,
        usertoken: token,
        q,
        page_size: String(limit),
        page: '1',
        format: 'json',
      });
      const url = `${MXM_BASE}/track.search?${params}`;
      const resp = await firstValueFrom(
        this.http.get<{
          message?: { body?: { track_list?: TrackSearchItem[] } };
        }>(url, { timeout: TIMEOUT_MS, headers: { Cookie: 'x-mxm-token-guid=' } }),
      );
      const list = resp.data?.message?.body?.track_list ?? [];
      return list
        .map((x) => x.track)
        .filter((t): t is NonNullable<typeof t> => !!t?.track_id)
        .map((t) => ({
          track_id: t.track_id as number,
          track_name: t.track_name,
          artist_name: t.artist_name,
          track_length: t.track_length,
          has_lyrics: t.has_lyrics ?? 0,
          has_subtitles: t.has_subtitles ?? 0,
        }));
    } catch (e) {
      this.logger.debug(`mxm track.search failed: ${(e as Error).message}`);
      return [];
    }
  }

  private async subtitleByTrackId(trackId: number, token: string): Promise<string | null> {
    try {
      const params = new URLSearchParams({
        app_id: APP_ID,
        usertoken: token,
        track_id: String(trackId),
        subtitle_format: 'lrc',
        format: 'json',
      });
      const url = `${MXM_BASE}/track.subtitle.get?${params}`;
      const resp = await firstValueFrom(
        this.http.get<{
          message?: { body?: { subtitle?: { subtitle_body?: string } } };
        }>(url, { timeout: TIMEOUT_MS, headers: { Cookie: 'x-mxm-token-guid=' } }),
      );
      const raw = resp.data?.message?.body?.subtitle?.subtitle_body;
      if (!raw) return null;
      const lrc = this.normalizeMxmSubtitle(raw);
      return lrc && lrc.length > 20 ? lrc : null;
    } catch (e) {
      this.logger.debug(`mxm track.subtitle failed: ${(e as Error).message}`);
      return null;
    }
  }

  private async lyricsByTrackId(trackId: number, token: string): Promise<string | null> {
    try {
      const params = new URLSearchParams({
        app_id: APP_ID,
        usertoken: token,
        track_id: String(trackId),
        format: 'json',
      });
      const url = `${MXM_BASE}/track.lyrics.get?${params}`;
      const resp = await firstValueFrom(
        this.http.get<{
          message?: { body?: { lyrics?: { lyrics_body?: string } } };
        }>(url, { timeout: TIMEOUT_MS, headers: { Cookie: 'x-mxm-token-guid=' } }),
      );
      const body = resp.data?.message?.body?.lyrics?.lyrics_body;
      if (!body || body.length < 20) return null;
      if (/this lyrics is not for commercial use|\*{5,}/i.test(body)) return null;
      return body.trim();
    } catch (e) {
      this.logger.debug(`mxm track.lyrics failed: ${(e as Error).message}`);
      return null;
    }
  }

  private async getToken(): Promise<string | null> {
    if (this.tokenCache && this.tokenCache.expiresAt > Date.now()) {
      return this.tokenCache.token;
    }
    try {
      const url = `${MXM_BASE}/token.get?app_id=${APP_ID}`;
      const resp = await firstValueFrom(
        this.http.get<{ message?: { body?: { user_token?: string } } }>(url, {
          timeout: TIMEOUT_MS,
          headers: { Cookie: 'x-mxm-token-guid=' },
        }),
      );
      const token = resp.data?.message?.body?.user_token;
      if (!token || token === 'UpgradeOnlyUpgradeOnlyUpgradeOnlyUpgradeOnly') {
        return null;
      }
      this.tokenCache = { token, expiresAt: Date.now() + TOKEN_TTL_MS };
      return token;
    } catch (e) {
      this.logger.debug(`mxm token.get failed: ${(e as Error).message}`);
      return null;
    }
  }

  private normalizeMxmSubtitle(raw: string): string {
    try {
      const parsed = JSON.parse(raw) as MxmSubtitle[];
      if (!Array.isArray(parsed)) return raw;
      return parsed
        .map((line) => {
          const total = line.time?.total ?? 0;
          const m = Math.floor(total / 60)
            .toString()
            .padStart(2, '0');
          const s = (total % 60).toFixed(2).padStart(5, '0');
          return `[${m}:${s}] ${line.text ?? ''}`;
        })
        .join('\n');
    } catch {
      return raw;
    }
  }
}
