import { Readable } from 'node:stream';
import { HttpService } from '@nestjs/axios';
import { Injectable, Logger } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import type { AxiosRequestConfig } from 'axios';
import { firstValueFrom } from 'rxjs';
import type { ScTokenResponse } from './soundcloud.types.js';

export interface OAuthCredentials {
  clientId: string;
  clientSecret: string;
  redirectUri: string;
}

@Injectable()
export class SoundcloudService {
  private readonly logger = new Logger(SoundcloudService.name);
  private readonly apiBaseUrl: string;
  private readonly authBaseUrl: string;
  private readonly defaultClientId: string;
  private readonly defaultRedirectUri: string;

  /**
   * CF proxy URL (напр. https://images.soundcloud.su).
   * Если задан — ВСЕ запросы к SC идут через этот URL с X-Target header.
   */
  private readonly proxyUrl: string;

  constructor(
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.apiBaseUrl = this.configService.get<string>('soundcloud.apiBaseUrl')!;
    this.authBaseUrl = this.configService.get<string>('soundcloud.authBaseUrl')!;
    this.defaultClientId = this.configService.get<string>('soundcloud.clientId')!;
    this.defaultRedirectUri = this.configService.get<string>('soundcloud.redirectUri')!;
    this.proxyUrl = this.configService.get<string>('soundcloud.proxyUrl') ?? '';

    if (this.proxyUrl) {
      this.logger.log(`CF proxy enabled: ${this.proxyUrl}`);
    }
  }

  get scAuthBaseUrl() {
    return this.authBaseUrl;
  }

  get scDefaultClientId() {
    return this.defaultClientId;
  }

  get scDefaultRedirectUri() {
    return this.defaultRedirectUri;
  }

  // ─── Proxy helpers ───────────────────────────────────────

  /**
   * Если proxyUrl задан — подменяет URL на proxy и добавляет X-Target header.
   * Если нет — возвращает original URL без изменений.
   */
  private proxyRewrite(originalUrl: string, headers: Record<string, string> = {}): {
    url: string;
    headers: Record<string, string>;
  } {
    if (!this.proxyUrl) {
      return { url: originalUrl, headers };
    }

    const encoded = Buffer.from(originalUrl).toString('base64');
    return {
      url: `${this.proxyUrl}/${encoded}`,
      headers: { ...headers, 'X-Target': encoded },
    };
  }

  // ─── Auth endpoints ──────────────────────────────────────

  async exchangeCodeForToken(
    code: string,
    codeVerifier: string,
    creds: OAuthCredentials,
  ): Promise<ScTokenResponse> {
    const targetUrl = `${this.authBaseUrl}/oauth/token`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

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
        {
          headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            Accept: 'application/json; charset=utf-8',
            ...proxyHeaders,
          },
        },
      ),
    );
    return data;
  }

  async refreshAccessToken(
    refreshToken: string,
    creds: OAuthCredentials,
  ): Promise<ScTokenResponse> {
    const targetUrl = `${this.authBaseUrl}/oauth/token`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

    const { data } = await firstValueFrom(
      this.httpService.post<ScTokenResponse>(
        url,
        new URLSearchParams({
          grant_type: 'refresh_token',
          client_id: creds.clientId,
          client_secret: creds.clientSecret,
          refresh_token: refreshToken,
        }).toString(),
        {
          headers: {
            'Content-Type': 'application/x-www-form-urlencoded',
            Accept: 'application/json; charset=utf-8',
            ...proxyHeaders,
          },
        },
      ),
    );
    return data;
  }

  async signOut(accessToken: string): Promise<void> {
    const targetUrl = `${this.authBaseUrl}/sign-out`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

    await firstValueFrom(
      this.httpService.post(
        url,
        JSON.stringify({ access_token: accessToken }),
        {
          headers: {
            'Content-Type': 'application/json; charset=utf-8',
            Accept: 'application/json; charset=utf-8',
            ...proxyHeaders,
          },
        },
      ),
    ).catch(() => {});
  }

  // ─── API methods ─────────────────────────────────────────

  async apiGet<T>(path: string, accessToken: string, params?: Record<string, unknown>): Promise<T> {
    const cleanParams = params
      ? Object.fromEntries(Object.entries(params).filter(([, v]) => v != null))
      : undefined;

    const targetUrl = `${this.apiBaseUrl}${path}`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

    const config: AxiosRequestConfig = {
      headers: {
        Authorization: `OAuth ${accessToken}`,
        Accept: 'application/json; charset=utf-8',
        ...proxyHeaders,
      },
      params: cleanParams,
    };

    const { data } = await firstValueFrom(this.httpService.get<T>(url, config));
    return data;
  }

  async apiPost<T>(
    path: string,
    accessToken: string,
    body?: unknown,
    config?: AxiosRequestConfig,
  ): Promise<T> {
    const targetUrl = `${this.apiBaseUrl}${path}`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

    const mergedConfig: AxiosRequestConfig = {
      ...config,
      headers: {
        Authorization: `OAuth ${accessToken}`,
        Accept: 'application/json; charset=utf-8',
        'Content-Type': 'application/json; charset=utf-8',
        ...config?.headers,
        ...proxyHeaders,
      },
    };

    const { data } = await firstValueFrom(
      this.httpService.post<T>(url, body, mergedConfig),
    );
    return data;
  }

  async apiPut<T>(
    path: string,
    accessToken: string,
    body?: unknown,
    config?: AxiosRequestConfig,
  ): Promise<T> {
    const targetUrl = `${this.apiBaseUrl}${path}`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

    const mergedConfig: AxiosRequestConfig = {
      ...config,
      headers: {
        Authorization: `OAuth ${accessToken}`,
        Accept: 'application/json; charset=utf-8',
        'Content-Type': 'application/json; charset=utf-8',
        ...config?.headers,
        ...proxyHeaders,
      },
    };

    const { data } = await firstValueFrom(
      this.httpService.put<T>(url, body, mergedConfig),
    );
    return data;
  }

  async apiDelete<T>(path: string, accessToken: string): Promise<T> {
    const targetUrl = `${this.apiBaseUrl}${path}`;
    const { url, headers: proxyHeaders } = this.proxyRewrite(targetUrl);

    const config: AxiosRequestConfig = {
      headers: {
        Authorization: `OAuth ${accessToken}`,
        Accept: 'application/json; charset=utf-8',
        ...proxyHeaders,
      },
      validateStatus: (status) => status >= 200 && status < 300,
    };

    const { data, status } = await firstValueFrom(
      this.httpService.delete<T>(url, config),
    );
    if (status === 204 || data === undefined || data === null || data === '') {
      return null as T;
    }
    return data;
  }

  // ─── Stream proxy ────────────────────────────────────────

  async proxyStream(
    streamUrl: string,
    accessToken: string,
    range?: string,
  ): Promise<{ stream: Readable; headers: Record<string, string> }> {
    const { url, headers: proxyHeaders } = this.proxyRewrite(streamUrl);

    const headers: Record<string, string> = {
      Authorization: `OAuth ${accessToken}`,
      ...proxyHeaders,
    };
    if (range) {
      headers.Range = range;
    }

    const { data, headers: resHeaders } = await firstValueFrom(
      this.httpService.get(url, {
        headers,
        responseType: 'stream',
        maxRedirects: 5,
      }),
    );

    const responseHeaders: Record<string, string> = {};
    for (const key of ['content-type', 'content-length', 'content-range', 'accept-ranges']) {
      if (resHeaders[key]) {
        responseHeaders[key] = String(resHeaders[key]);
      }
    }

    return { stream: data as Readable, headers: responseHeaders };
  }
}
