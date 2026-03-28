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
    range?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> }> {
    return this.sc.proxyStream(url, token, range);
  }

  // ─── Stream with CDN ─────────────────────────────────────

  async getStreamWithCdn(
    token: string,
    trackUrn: string,
    format: string,
    params: Record<string, unknown>,
    range?: string,
    hq?: boolean,
  ): Promise<
    | { type: 'redirect'; url: string }
    | { type: 'stream'; stream: Readable; headers: Record<string, string> }
    | null
  > {
    // 1. Проверяем CDN (redirect — CDN сам обработает Range)
    if (this.cdn.enabled) {
      const cdnResult = await this.tryServFromCdn(trackUrn, hq ?? false);
      if (cdnResult) return cdnResult;
    }

    // 2. Качаем с SC
    let access: 'playable' | 'preview' | 'blocked' = 'playable';
    try {
      const track = await this.sc.apiGet<ScTrack>(`/tracks/${trackUrn}`, token, params);
      access = track.access;
    } catch (err) {
      console.log(err);
    }

    const streamData = await this.fetchFromSc(
      token,
      trackUrn,
      format,
      params,
      range,
      hq ?? false,
      access,
    );
    if (!streamData) return null;

    // 3. Если CDN включён и полный стрим (не partial) — tee на CDN
    if (this.cdn.enabled && !range) {
      return this.teeStreamToCdn(trackUrn, streamData);
    }

    return { type: 'stream', stream: streamData.stream, headers: streamData.headers };
  }

  /**
   * Пытается отдать трек с CDN.
   * Если клиент хочет hq а на CDN только sq — проверяет доступность HQ.
   */
  private async tryServFromCdn(
    trackUrn: string,
    wantHq: boolean,
  ): Promise<
    | { type: 'redirect'; url: string }
    | { type: 'stream'; stream: Readable; headers: Record<string, string> }
    | null
  > {
    const cached = await this.cdn.findCachedTrack(trackUrn, wantHq);
    if (!cached) return null;

    // Клиент хочет HQ, но на CDN только SQ
    if (wantHq && cached.quality === CdnQuality.SQ) {
      const hqAvailable = cached.hqAvailable ?? (await this.cdn.getHqAvailable(trackUrn));

      if (hqAvailable === null) {
        // Не проверяли — проверяем через cookie-client
        const hasHq = await this.scPublicCookies.checkHqAvailable(trackUrn);
        await this.cdn.setHqAvailable(trackUrn, hasHq);

        if (hasHq) {
          // Есть HQ на SC — стримим HQ клиенту + загружаем на CDN
          const hqStream = await this.getCookieStream(trackUrn);
          if (hqStream && hqStream.quality === 'hq') {
            return this.teeStreamToCdn(trackUrn, hqStream);
          }
          // Не удалось получить HQ стрим — отдаём SQ с CDN
        }
        // HQ недоступен — отдаём SQ
      }
      // hqAvailable === false — отдаём SQ с CDN
    }

    // Верифицируем что CDN реально отдаёт файл
    const cdnUrl = this.cdn.getCdnUrl(trackUrn, cached.quality as CdnQuality);
    const alive = await this.cdn.verifyCdnUrl(cdnUrl);
    if (!alive) {
      await this.cdn.markError(cached.id);
      return null;
    }

    return { type: 'redirect', url: cdnUrl };
  }

  /** Качает стрим с SC с fallback-логикой */
  private async fetchFromSc(
    token: string,
    trackUrn: string,
    format: string,
    params: Record<string, unknown>,
    range: string | undefined,
    hq: boolean,
    access: string,
  ): Promise<StreamResult | null> {
    if (hq || access !== 'playable') {
      // hq=true → куки ДО апи
      let data = await this.getCookieStream(trackUrn);
      if (!data) {
        const oauthData = await this.tryOAuthStream(token, trackUrn, format, params, range);
        data = oauthData ? { ...oauthData, quality: 'sq' as const } : null;
      }
      if (!data) {
        const pubData = await this.getPublicStream(trackUrn, format);
        data = pubData ? { ...pubData, quality: 'sq' as const } : null;
      }
      return data;
    }

    // default → апи → анонимная сессия → куки
    const oauthData = await this.tryOAuthStream(token, trackUrn, format, params, range);
    if (oauthData) return { ...oauthData, quality: 'sq' as const };

    const pubData = await this.getPublicStream(trackUrn, format);
    if (pubData) return { ...pubData, quality: 'sq' as const };

    const cookieData = await this.getCookieStream(trackUrn);
    return cookieData ?? null;
  }

  /** Tee: один стрим клиенту, буфер на CDN */
  private teeStreamToCdn(
    trackUrn: string,
    streamData: StreamResult,
  ): { type: 'stream'; stream: Readable; headers: Record<string, string> } {
    const { stream, headers, quality } = streamData;
    const cdnQuality = quality === 'hq' ? CdnQuality.HQ : CdnQuality.SQ;
    const clientStream = new PassThrough();
    const cdnChunks: Buffer[] = [];

    stream.on('data', (chunk: Buffer) => {
      clientStream.write(chunk);
      cdnChunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    });
    stream.on('end', () => {
      clientStream.end();
      const buffer = Buffer.concat(cdnChunks);
      if (buffer.length > 8192) {
        this.cdn.uploadWithTracking(trackUrn, cdnQuality, buffer).catch((err) => {
          this.logger.warn(`CDN upload failed for ${trackUrn}: ${err.message}`);
        });
      }
    });
    stream.on('error', (err) => {
      clientStream.destroy(err);
    });

    return { type: 'stream', stream: clientStream, headers };
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
        'hls_aac_160_url',
        'http_mp3_128_url',
        'hls_mp3_128_url',
      ];

      // Build ordered list: requested format first, then fallbacks
      const candidates: { key: keyof ScStreams; url: string }[] = [];
      const requestedUrl = streams[urlKey] as string | undefined;
      if (requestedUrl) {
        candidates.push({ key: urlKey as keyof ScStreams, url: requestedUrl });
      }
      for (const key of fallbackOrder) {
        if (streams[key] && key !== urlKey) {
          candidates.push({ key, url: streams[key] as string });
        }
      }

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
          return await this.proxyStream(token, url, range);
        } catch (err: any) {
          this.logger.warn(`Stream format ${fmt} failed: ${err.message}, trying next...`);
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
