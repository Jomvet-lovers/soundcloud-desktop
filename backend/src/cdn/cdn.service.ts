import { Readable } from 'node:stream';
import { HttpService } from '@nestjs/axios';
import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { firstValueFrom } from 'rxjs';

/**
 * Сервис для кэширования аудио-треков на SecureServe CDN.
 * Если CDN не настроен (env пустые) — все методы no-op.
 */
@Injectable()
export class CdnService implements OnModuleInit {
  private readonly logger = new Logger(CdnService.name);
  private readonly baseUrl: string;
  private readonly authToken: string;

  get enabled(): boolean {
    return !!(this.baseUrl && this.authToken);
  }

  constructor(
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.baseUrl = (this.configService.get<string>('cdn.baseUrl') ?? '').replace(/\/+$/, '');
    this.authToken = this.configService.get<string>('cdn.authToken') ?? '';
  }

  onModuleInit() {
    if (this.enabled) {
      this.logger.log(`CDN enabled: ${this.baseUrl}`);
    } else {
      this.logger.log('CDN disabled (CDN_BASE_URL / CDN_AUTH_TOKEN not set)');
    }
  }

  /** Путь файла на CDN */
  private trackPath(trackUrn: string): string {
    return `audio/${trackUrn.replace(/:/g, '_')}.mp3`;
  }

  /** Публичный URL трека на CDN */
  getCdnUrl(trackUrn: string): string {
    return `${this.baseUrl}/${this.trackPath(trackUrn)}`;
  }

  /** Проверяет наличие трека на CDN (HEAD запрос) */
  async isOnCdn(trackUrn: string): Promise<boolean> {
    if (!this.enabled) return false;

    try {
      const { status } = await firstValueFrom(
        this.httpService.head(this.getCdnUrl(trackUrn), {
          validateStatus: () => true,
          timeout: 3000,
        }),
      );
      return status === 200;
    } catch {
      return false;
    }
  }

  /**
   * Загружает аудио-буфер на CDN через двухфазный signed upload.
   * Вызывается fire-and-forget — не блокирует ответ клиенту.
   */
  async uploadToCdn(trackUrn: string, audioBuffer: Buffer): Promise<boolean> {
    if (!this.enabled) return false;

    const path = this.trackPath(trackUrn);
    const uploadToken = `sc-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;

    try {
      // Phase 1: Sign upload
      const signRes = await firstValueFrom(
        this.httpService.post(
          `${this.baseUrl}/sign-upload`,
          {
            token: uploadToken,
            path,
            size: audioBuffer.length,
            content_type: 'audio/mpeg',
          },
          {
            headers: {
              Authorization: this.authToken,
              'Content-Type': 'application/json',
            },
            timeout: 5000,
          },
        ),
      );

      if (signRes.status !== 200) {
        this.logger.warn(`CDN sign-upload failed: ${signRes.status}`);
        return false;
      }

      // Phase 2: Upload file (multipart)
      const FormData = (await import('form-data')).default;
      const form = new FormData();
      form.append('token', uploadToken);
      form.append('file', audioBuffer, {
        filename: 'track.mp3',
        contentType: 'audio/mpeg',
      });

      const uploadRes = await firstValueFrom(
        this.httpService.post(`${this.baseUrl}/upload`, form, {
          headers: {
            Authorization: this.authToken,
            ...form.getHeaders(),
          },
          timeout: 30000,
          maxBodyLength: Infinity,
        }),
      );

      if (uploadRes.status !== 200) {
        this.logger.warn(`CDN upload failed: ${uploadRes.status}`);
        return false;
      }

      this.logger.log(`CDN uploaded: ${path} (${(audioBuffer.length / 1024 / 1024).toFixed(1)}MB)`);
      return true;
    } catch (err: any) {
      this.logger.warn(`CDN upload error for ${trackUrn}: ${err.message}`);
      return false;
    }
  }

  /**
   * Скачивает stream и собирает его в Buffer.
   * Используется для параллельной загрузки на CDN.
   */
  async streamToBuffer(stream: Readable): Promise<Buffer> {
    const chunks: Buffer[] = [];
    for await (const chunk of stream) {
      chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk));
    }
    return Buffer.concat(chunks);
  }
}
