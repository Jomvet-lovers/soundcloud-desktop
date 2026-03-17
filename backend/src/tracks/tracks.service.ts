import type { Readable } from 'node:stream';
import { Injectable, Logger } from '@nestjs/common';
import { ScPublicApiService } from '../soundcloud/sc-public-api.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import type {
  ScComment,
  ScPaginatedResponse,
  ScStreams,
  ScTrack,
  ScUser,
} from '../soundcloud/soundcloud.types.js';

@Injectable()
export class TracksService {
  private readonly logger = new Logger(TracksService.name);

  constructor(
    private readonly sc: SoundcloudService,
    private readonly scPublicApi: ScPublicApiService,
  ) {}

  search(token: string, params?: Record<string, unknown>): Promise<ScPaginatedResponse<ScTrack>> {
    return this.sc.apiGet('/tracks', token, params);
  }

  getById(token: string, trackUrn: string, params?: Record<string, unknown>): Promise<ScTrack> {
    return this.sc.apiGet(`/tracks/${trackUrn}`, token, params);
  }

  update(token: string, trackUrn: string, body: unknown): Promise<ScTrack> {
    return this.sc.apiPut(`/tracks/${trackUrn}`, token, body);
  }

  delete(token: string, trackUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/tracks/${trackUrn}`, token);
  }

  getStreams(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScStreams> {
    return this.sc.apiGet(`/tracks/${trackUrn}/streams`, token, params);
  }

  proxyStream(
    token: string,
    url: string,
    range?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> }> {
    return this.sc.proxyStream(url, token, range);
  }

  async tryOAuthStream(
    token: string,
    trackUrn: string,
    format: string,
    params: Record<string, unknown>,
    range?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> } | null> {
    try {
      const streams = await this.getStreams(token, trackUrn, params);
      const urlKey = `${format}_url` as keyof typeof streams;

      const fallbackOrder: (keyof ScStreams)[] = [
        'hls_aac_160_url', 'http_mp3_128_url',
        'hls_mp3_128_url', 'preview_mp3_128_url',
      ];
      let streamUrl = streams[urlKey] as string | undefined;
      let actualKey = streamUrl ? urlKey : undefined;

      if (!streamUrl) {
        for (const key of fallbackOrder) {
          if (streams[key]) {
            streamUrl = streams[key] as string;
            actualKey = key;
            break;
          }
        }
      }

      if (!streamUrl || !actualKey) return null;

      const actualFormat = (actualKey as string).replace('_url', '');
      const isHls = actualFormat.startsWith('hls_');

      if (isHls) {
        return await this.scPublicApi.streamFromHls(streamUrl, this.hlsMimeType(actualFormat));
      }

      return await this.proxyStream(token, streamUrl, range);
    } catch {
      return null;
    }
  }

  private hlsMimeType(format: string): string {
    if (format.includes('aac')) return 'audio/mp4; codecs="mp4a.40.2"';
    if (format.includes('opus')) return 'audio/ogg; codecs="opus"';
    return 'audio/mpeg';
  }

  /**
   * Fallback: resolve stream via SoundCloud public API (no OAuth).
   * Used when the authenticated /streams endpoint fails or returns empty.
   */
  async getPublicStream(
    trackUrn: string,
    format?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> } | null> {
    try {
      return await this.scPublicApi.getStreamForTrack(trackUrn, format);
    } catch (err: any) {
      this.logger.warn(`Public API fallback failed for ${trackUrn}: ${err.message}`);
      return null;
    }
  }

  getComments(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScComment>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/comments`, token, params);
  }

  createComment(
    token: string,
    trackUrn: string,
    body: { comment: { body: string; timestamp?: number } },
  ): Promise<ScComment> {
    return this.sc.apiPost(`/tracks/${trackUrn}/comments`, token, body);
  }

  getFavoriters(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/favoriters`, token, params);
  }

  getReposters(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/reposters`, token, params);
  }

  getRelated(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/related`, token, params);
  }
}