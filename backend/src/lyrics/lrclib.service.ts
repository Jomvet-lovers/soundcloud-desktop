import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { firstValueFrom } from 'rxjs';

const LRCLIB_API = 'https://lrclib.net/api';
const TIMEOUT_MS = 15000;

export interface LrclibResult {
  syncedLrc: string | null;
  plainText: string | null;
  artistGuess?: string;
  titleGuess?: string;
  durationSec?: number;
}

interface LrclibRaw {
  syncedLyrics?: string;
  plainLyrics?: string;
  artistName?: string;
  trackName?: string;
  duration?: number;
}

@Injectable()
export class LrclibService {
  private readonly logger = new Logger(LrclibService.name);

  constructor(private readonly http: HttpService) {}

  async searchByQuery(q: string, limit = 3): Promise<LrclibResult[]> {
    try {
      const url = `${LRCLIB_API}/search?${new URLSearchParams({ q })}`;
      const resp = await firstValueFrom(this.http.get<LrclibRaw[]>(url, { timeout: TIMEOUT_MS }));
      const data = resp.data;
      if (!Array.isArray(data) || !data.length) return [];
      return data
        .slice(0, limit)
        .filter((e) => e.syncedLyrics || e.plainLyrics)
        .map((e) => ({
          syncedLrc: e.syncedLyrics ?? null,
          plainText: e.plainLyrics ?? null,
          artistGuess: e.artistName,
          titleGuess: e.trackName,
          durationSec: typeof e.duration === 'number' ? e.duration : undefined,
        }));
    } catch (e) {
      this.logger.debug(`LRCLIB search failed: ${(e as Error).message}`);
      return [];
    }
  }
}
