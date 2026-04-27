import { Readable } from 'node:stream';
import { HttpService } from '@nestjs/axios';
import { Injectable } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import type { AxiosRequestConfig } from 'axios';
import { firstValueFrom } from 'rxjs';
import type { ScTokenResponse } from './soundcloud.types.js';

export interface OAuthCredentials {
  clientId: string;
  clientSecret: string;
  redirectUri: string;
}

const API_BASE = 'https://api.soundcloud.com';
const AUTH_BASE = 'https://secure.soundcloud.com';

@Injectable()
export class SoundcloudService {
  private readonly defaultClientId: string;
  private readonly defaultRedirectUri: string;
  private readonly apiProxyUrl: string;
  private readonly proxyFallback: boolean;

  constructor(
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.defaultClientId = this.configService.get<string>('soundcloud.clientId')!;
    this.defaultRedirectUri = this.configService.get<string>('soundcloud.redirectUri')!;
    this.apiProxyUrl = this.configService.get<string>('soundcloud.proxyUrl') ?? '';
    this.proxyFallback = this.configService.get<boolean>('soundcloud.proxyFallback') ?? false;
  }

  get scAuthBaseUrl() {
    return AUTH_BASE;
  }

  get scDefaultClientId() {
    return this.defaultClientId;
  }

  get scDefaultRedirectUri() {
    return this.defaultRedirectUri;
  }

  get scApiProxyUrl() {
    return this.apiProxyUrl;
  }

  /**
   * If proxyUrl is set, rewrites the request to go through CF Worker:
   * - URL becomes proxyUrl (no path)
   * - X-Target header = base64(originalUrl)
   */
  private proxyWith(
    proxyUrl: string,
    targetUrl: string,
    extra: Record<string, string> = {},
  ): {
    url: string;
    headers: Record<string, string>;
  } {
    if (!proxyUrl) {
      return { url: targetUrl, headers: extra };
    }
    return {
      url: proxyUrl,
      headers: { ...extra, 'X-Target': Buffer.from(targetUrl).toString('base64') },
    };
  }

  // ─── Auth ──────────────────────────────────────────────────

  async exchangeCodeForToken(
    code: string,
    codeVerifier: string,
    creds: OAuthCredentials,
  ): Promise<ScTokenResponse> {
    const { url, headers } = this.proxyWith(this.apiProxyUrl, `${AUTH_BASE}/oauth/token`, {
      'Content-Type': 'application/x-www-form-urlencoded',
      Accept: 'application/json; charset=utf-8',
    });

    const { data } = await firstValueFrom(
      this.httpService.post<ScTokenResponse>(
        url,
        new URLSearchParams({
          grant_type: 'authorization_code',
          client_id: creds.clientId,
          client_secret: creds.clientSecret,
          code,
          redirect_uri: creds.redirectUri,
          code_verifier: codeVerifier,
        }).toString(),
        { headers },
      ),
    );
    return data;
  }

  async refreshAccessToken(
    refreshToken: string,
    creds: OAuthCredentials,
  ): Promise<ScTokenResponse> {
    const { url, headers } = this.proxyWith(this.apiProxyUrl, `${AUTH_BASE}/oauth/token`, {
      'Content-Type': 'application/x-www-form-urlencoded',
      Accept: 'application/json; charset=utf-8',
    });

    const { data } = await firstValueFrom(
      this.httpService.post<ScTokenResponse>(
        url,
        new URLSearchParams({
          grant_type: 'refresh_token',
          client_id: creds.clientId,
          client_secret: creds.clientSecret,
          refresh_token: refreshToken,
        }).toString(),
        { headers },
      ),
    );
    return data;
  }

  async signOut(accessToken: string): Promise<void> {
    const { url, headers } = this.proxyWith(this.apiProxyUrl, `${AUTH_BASE}/sign-out`, {
      'Content-Type': 'application/json; charset=utf-8',
      Accept: 'application/json; charset=utf-8',
    });

    await firstValueFrom(
      this.httpService.post(url, JSON.stringify({ access_token: accessToken }), { headers }),
    ).catch(() => {});
  }

  // ─── Fallback helper ────────────────────────────────────────

  private async withFallback<T>(
    targetUrl: string,
    extraHeaders: Record<string, string>,
    fn: (url: string, headers: Record<string, string>) => Promise<T>,
  ): Promise<T> {
    if (this.proxyFallback && this.apiProxyUrl) {
      try {
        return await fn(targetUrl, extraHeaders);
      } catch {
        const { url, headers } = this.proxyWith(this.apiProxyUrl, targetUrl, extraHeaders);
        return fn(url, headers);
      }
    }
    const { url, headers } = this.proxyWith(this.apiProxyUrl, targetUrl, extraHeaders);
    return fn(url, headers);
  }

  // ─── API ───────────────────────────────────────────────────

  async apiGet<T>(
    path: string,
    accessToken: string,
    params?: Record<string, unknown>,
    options?: { skipTrackDiscovery?: boolean },
  ): Promise<T> {
    const cleanParams = params
      ? Object.fromEntries(Object.entries(params).filter(([, v]) => v != null))
      : undefined;

    const target = new URL(`${API_BASE}${path}`);
    if (cleanParams) {
      for (const [k, v] of Object.entries(cleanParams)) {
        target.searchParams.set(k, String(v));
      }
    }

    const extraHeaders = {
      Authorization: `OAuth ${accessToken}`,
      Accept: 'application/json; charset=utf-8',
    };

    const extraConfig: AxiosRequestConfig = {};
    if (options?.skipTrackDiscovery) {
      (extraConfig as AxiosRequestConfig & { skipTrackDiscovery?: boolean }).skipTrackDiscovery =
        true;
    }

    return this.withFallback(target.toString(), extraHeaders, async (url, headers) => {
      const { data } = await firstValueFrom(
        this.httpService.get<T>(url, { headers, ...extraConfig }),
      );
      return data;
    });
  }

  async apiPost<T>(
    path: string,
    accessToken: string,
    body?: unknown,
    config?: AxiosRequestConfig,
  ): Promise<T> {
    const targetUrl = `${API_BASE}${path}`;
    const extraHeaders = {
      Authorization: `OAuth ${accessToken}`,
      Accept: 'application/json; charset=utf-8',
      'Content-Type': 'application/json; charset=utf-8',
      ...(config?.headers as Record<string, string>),
    };

    return this.withFallback(targetUrl, extraHeaders, async (url, headers) => {
      const { data } = await firstValueFrom(this.httpService.post<T>(url, body, { headers }));
      return data;
    });
  }

  async apiPut<T>(
    path: string,
    accessToken: string,
    body?: unknown,
    config?: AxiosRequestConfig,
  ): Promise<T> {
    const targetUrl = `${API_BASE}${path}`;
    const extraHeaders = {
      Authorization: `OAuth ${accessToken}`,
      Accept: 'application/json; charset=utf-8',
      'Content-Type': 'application/json; charset=utf-8',
      ...(config?.headers as Record<string, string>),
    };

    return this.withFallback(targetUrl, extraHeaders, async (url, headers) => {
      const { data } = await firstValueFrom(this.httpService.put<T>(url, body, { headers }));
      return data;
    });
  }

  async apiDelete<T>(path: string, accessToken: string): Promise<T> {
    const targetUrl = `${API_BASE}${path}`;
    const extraHeaders = {
      Authorization: `OAuth ${accessToken}`,
      Accept: 'application/json; charset=utf-8',
    };

    return this.withFallback(targetUrl, extraHeaders, async (url, headers) => {
      const { data, status } = await firstValueFrom(
        this.httpService.delete<T>(url, {
          headers,
          validateStatus: (s) => s >= 200 && s < 300,
        }),
      );
      return status === 204 || data == null || data === '' ? (null as T) : data;
    });
  }

  // ─── Stream ────────────────────────────────────────────────

  async proxyStream(
    streamUrl: string,
    accessToken: string,
    range?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> }> {
    const extra: Record<string, string> = { Authorization: `OAuth ${accessToken}` };
    if (range) extra.Range = range;

    return this.withFallback(streamUrl, extra, async (url, headers) => {
      const { data, headers: resHeaders } = await firstValueFrom(
        this.httpService.get(url, { headers, responseType: 'stream', maxRedirects: 5 }),
      );
      const responseHeaders: Record<string, string> = {};
      for (const key of ['content-type', 'content-length', 'content-range', 'accept-ranges']) {
        if (resHeaders[key]) responseHeaders[key] = String(resHeaders[key]);
      }
      return { stream: data as Readable, headers: responseHeaders };
    });
  }
}
