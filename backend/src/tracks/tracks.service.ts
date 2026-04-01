import { createWriteStream, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { PassThrough, type Readable } from 'node:stream';
import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { CdnService } from '../cdn/cdn.service.js';
import { CdnQuality } from '../cdn/entities/cdn-track.entity.js';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { PendingActionsService } from '../pending-actions/pending-actions.service.js';
import { ScPublicAnonService } from '../soundcloud/sc-public-anon.service.js';
import { ScPublicCookiesService } from '../soundcloud/sc-public-cookies.service.js';
import { streamFromHls } from '../soundcloud/sc-public-utils.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import type {
  ScComment,
  ScPaginatedResponse,
  ScStreams,
  ScTrack,
  ScUser,
} from '../soundcloud/soundcloud.types.js';

interface StreamResult {
  stream: Readable;
  headers: Record<string, string>;
  quality: 'hq' | 'sq';
}

@Injectable()
export class TracksService {
  private readonly logger = new Logger(TracksService.name);

  constructor(
    private readonly sc: SoundcloudService,
    private readonly scPublicAnon: ScPublicAnonService,
    private readonly scPublicCookies: ScPublicCookiesService,
    private readonly localLikes: LocalLikesService,
    private readonly cdn: CdnService,
    private readonly pendingActions: PendingActionsService,
    private readonly httpService: HttpService,
  ) {}

  private async applyLocalLikeFlags(sessionId: string, tracks: ScTrack[]): Promise<ScTrack[]> {
    const urns = tracks.map((track) => track.urn).filter(Boolean);
    const likedUrns = await this.localLikes.getLikedTrackIds(sessionId, urns);
    if (likedUrns.size === 0) {
      return tracks;
    }

    return tracks.map((track) =>
      likedUrns.has(track.urn) ? { ...track, user_favorite: true } : track,
    );
  }

  async search(
    token: string,
    sessionId: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    const response = await this.sc.apiGet<ScPaginatedResponse<ScTrack>>('/tracks', token, params);
    response.collection = await this.applyLocalLikeFlags(sessionId, response.collection ?? []);
    return response;
  }

  async getById(
    token: string,
    sessionId: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScTrack> {
    const track = await this.sc.apiGet<ScTrack>(`/tracks/${trackUrn}`, token, params);
    const [annotated] = await this.applyLocalLikeFlags(sessionId, [track]);
    return annotated;
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
  ): Promise<{ stream: Readable; headers: Record<string, string> }> {
    return this.sc.proxyStream(url, token);
  }

  // ─── Stream ──────────────────────────────────────────────

  /**
   * Получить аудио-стрим для трека.
   * Приоритет: CDN → cookies HQ → OAuth API → anon.
   * Если CDN включён и стрим получен не с CDN — tee на диск для загрузки.
   */
  async getStream(
    token: string,
    trackUrn: string,
    params: Record<string, unknown>,
    hq: boolean,
  ): Promise<
    | { type: 'redirect'; url: string }
    | { type: 'stream'; stream: Readable; headers: Record<string, string> }
    | null
  > {
    let skipCdnUpload = this.cdn.isTemporarilyUnavailable();

    // 1. CDN
    if (this.cdn.enabled) {
      const cdnResult = await this.tryServFromCdn(trackUrn);
      if (cdnResult) {
        if (cdnResult.type === 'redirect') {
          this.logger.log(`[stream] ${trackUrn} → CDN`);
          return cdnResult;
        }
        skipCdnUpload = true;
      }
    }

    // 2. Получаем стрим с SC
    const streamData = await this.fetchFromSc(token, trackUrn, params, hq);
    if (!streamData) {
      this.logger.warn(`[stream] ${trackUrn} → no source available`);
      return null;
    }

    this.logger.log(`[stream] ${trackUrn} → ${streamData.quality} via ${streamData.source}`);

    // 3. Tee на CDN если включён
    if (this.cdn.enabled && !skipCdnUpload && !this.cdn.isTemporarilyUnavailable()) {
      return this.teeStreamToCdn(trackUrn, streamData);
    }

    return { type: 'stream', stream: streamData.stream, headers: streamData.headers };
  }

  /** Пытается отдать трек с CDN (HQ приоритет, потом SQ). */
  private async tryServFromCdn(
    trackUrn: string,
  ): Promise<{ type: 'redirect'; url: string } | { type: 'unavailable' } | null> {
    const cached = await this.cdn.findCachedTrack(trackUrn, true);
    if (!cached) return null;

    const cdnUrl = this.cdn.getCdnUrl(trackUrn, cached.quality as CdnQuality);
    const verifyResult = await this.cdn.verifyCdnUrl(cdnUrl);

    if (verifyResult === 'ok') {
      return { type: 'redirect', url: cdnUrl };
    }

    if (verifyResult === 'missing') {
      await this.cdn.markError(cached.id);
    }

    if (verifyResult === 'unavailable') {
      return { type: 'unavailable' };
    }

    return null;
  }

  /**
   * Качает стрим с SC.
   * hq=true:  cookies HQ → OAuth → anon
   * hq=false: OAuth → anon → cookies
   */
  private async fetchFromSc(
    token: string,
    trackUrn: string,
    params: Record<string, unknown>,
    hq: boolean,
  ): Promise<(StreamResult & { source: string }) | null> {
    if (hq) {
      const cookieData = await this.getCookieStream(trackUrn);
      if (cookieData) return { ...cookieData, source: 'cookies' };

      const oauthData = await this.tryOAuthStream(token, trackUrn, params);
      if (oauthData) return { ...oauthData, quality: 'sq', source: 'oauth' };

      const anonData = await this.getPublicStream(trackUrn);
      if (anonData) return { ...anonData, quality: 'sq', source: 'anon' };
    } else {
      const oauthData = await this.tryOAuthStream(token, trackUrn, params);
      if (oauthData) return { ...oauthData, quality: 'sq', source: 'oauth' };

      const anonData = await this.getPublicStream(trackUrn);
      if (anonData) return { ...anonData, quality: 'sq', source: 'anon' };

      const cookieData = await this.getCookieStream(trackUrn);
      if (cookieData) return { ...cookieData, source: 'cookies' };
    }

    return null;
  }

  /** Tee: стрим клиенту + pipe на диск для CDN upload. Не буферизует в RAM. */
  private teeStreamToCdn(
    trackUrn: string,
    streamData: StreamResult & { source: string },
  ): { type: 'stream'; stream: Readable; headers: Record<string, string> } {
    const { stream, headers, quality } = streamData;
    const cdnQuality = quality === 'hq' ? CdnQuality.HQ : CdnQuality.SQ;
    const tmpFile = join(
      tmpdir(),
      `cdn-${Date.now()}-${Math.random().toString(36).slice(2, 8)}.mp3`,
    );
    const fileStream = createWriteStream(tmpFile);
    const clientStream = new PassThrough();

    // pipe source → clientStream (для клиента) + fileStream (для CDN)
    // Ни один чанк не копируется в отдельный буфер
    stream.on('data', (chunk: Buffer) => {
      clientStream.write(chunk);
      fileStream.write(chunk);
    });

    stream.on('end', () => {
      clientStream.end();
      fileStream.end();
    });

    // Upload на CDN после того как файл полностью записан на диск
    fileStream.on('finish', () => {
      const size = statSync(tmpFile).size;
      if (size > 8192) {
        this.logger.log(`[stream] ${trackUrn} → CDN upload (${(size / 1024).toFixed(0)} KB)`);
        this.cdn.uploadWithTracking(trackUrn, cdnQuality, tmpFile).catch((err) => {
          this.logger.warn(`CDN upload failed for ${trackUrn}: ${err.message}`);
        });
      }
    });

    stream.on('error', (err) => {
      clientStream.destroy(err);
      fileStream.destroy();
    });

    return { type: 'stream', stream: clientStream, headers };
  }

  /** OAuth API stream: пробует форматы по приоритету. */
  private async tryOAuthStream(
    token: string,
    trackUrn: string,
    params: Record<string, unknown>,
  ): Promise<{ stream: Readable; headers: Record<string, string> } | null> {
    try {
      const streams = await this.getStreams(token, trackUrn, params);

      const formatOrder: (keyof ScStreams)[] = [
        'hls_aac_160_url',
        'http_mp3_128_url',
        'hls_mp3_128_url',
      ];

      const candidates = formatOrder
        .filter((key) => !!streams[key])
        .map((key) => ({ key, url: streams[key] as string }));

      if (!candidates.length) return null;

      for (const { key, url } of candidates) {
        const fmt = (key as string).replace('_url', '');
        const isHls = fmt.startsWith('hls_');

        try {
          if (isHls) {
            return await streamFromHls(
              this.httpService,
              this.sc.scApiProxyUrl,
              url,
              this.hlsMimeType(fmt),
            );
          }
          return await this.proxyStream(token, url);
        } catch (err: any) {
          this.logger.warn(`[stream] format ${fmt} failed: ${err.message}`);
        }
      }

      return null;
    } catch {
      return null;
    }
  }

  private hlsMimeType(format: string): string {
    if (format.includes('aac')) return 'audio/mp4; codecs="mp4a.40.2"';
    if (format.includes('opus')) return 'audio/ogg; codecs="opus"';
    return 'audio/mpeg';
  }

  async getCookieStream(trackUrn: string): Promise<StreamResult | null> {
    if (!this.scPublicCookies.hasCookies) return null;
    try {
      const result = await this.scPublicCookies.getStreamViaCookies(trackUrn);
      if (!result) return null;
      return {
        stream: result.stream as Readable,
        headers: result.headers,
        quality: result.quality,
      };
    } catch (err: any) {
      this.logger.warn(`Cookie stream failed for ${trackUrn}: ${err.message}`);
      return null;
    }
  }

  async getPublicStream(
    trackUrn: string,
    format?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> } | null> {
    try {
      return await this.scPublicAnon.getStreamForTrack(trackUrn, format);
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

  async createComment(
    token: string,
    sessionId: string,
    trackUrn: string,
    body: { comment: { body: string; timestamp?: number } },
  ): Promise<unknown> {
    try {
      return await this.sc.apiPost<ScComment>(`/tracks/${trackUrn}/comments`, token, body);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(
          sessionId,
          'comment',
          trackUrn,
          body as unknown as Record<string, unknown>,
        );
        return { queued: true, actionType: 'comment', targetUrn: trackUrn };
      }
      throw error;
    }
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

  async getRelated(
    token: string,
    sessionId: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    const response = await this.sc.apiGet<ScPaginatedResponse<ScTrack>>(
      `/tracks/${trackUrn}/related`,
      token,
      params,
    );
    response.collection = await this.applyLocalLikeFlags(sessionId, response.collection ?? []);
    return response;
  }
}
