import type { AxiosResponse } from 'axios';
import { HttpService } from '@nestjs/axios';
import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { createReadStream, statSync } from 'node:fs';
import { unlink } from 'node:fs/promises';
import { firstValueFrom } from 'rxjs';
import { Repository } from 'typeorm';
import { CdnQuality, CdnStatus, CdnTrack } from './entities/cdn-track.entity.js';

export type CdnVerifyResult = 'ok' | 'missing' | 'unavailable';
type CdnUploadResult = 'uploaded' | 'failed' | 'unavailable';

@Injectable()
export class CdnService implements OnModuleInit {
  private readonly logger = new Logger(CdnService.name);
  private readonly baseUrl: string;
  private readonly authToken: string;
  private readonly uploadTimeoutMs: number;
  private readonly unavailableThreshold: number;
  private readonly unavailableCooldownMs: number;
  private consecutiveUnavailable = 0;
  private unavailableUntil = 0;

  get enabled(): boolean {
    return !!(this.baseUrl && this.authToken);
  }

  constructor(
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
    @InjectRepository(CdnTrack)
    private readonly cdnTrackRepo: Repository<CdnTrack>,
  ) {
    this.baseUrl = (this.configService.get<string>('cdn.baseUrl') ?? '').replace(/\/+$/, '');
    this.authToken = this.configService.get<string>('cdn.authToken') ?? '';
    this.uploadTimeoutMs = this.configService.get<number>('cdn.uploadTimeoutMs') ?? 600_000;
    this.unavailableThreshold = Math.max(
      1,
      this.configService.get<number>('cdn.unavailableThreshold') ?? 3,
    );
    this.unavailableCooldownMs = Math.max(
      1_000,
      this.configService.get<number>('cdn.unavailableCooldownMs') ?? 60_000,
    );
  }

  onModuleInit() {
    if (this.enabled) {
      this.logger.log(`CDN enabled: ${this.baseUrl}`);
    } else {
      this.logger.log('CDN disabled (CDN_BASE_URL / CDN_AUTH_TOKEN not set)');
    }
  }

  /** Путь файла на CDN: hq/soundcloud_tracks_123.mp3 или sq/... */
  trackPath(trackUrn: string, quality: CdnQuality): string {
    return `${quality}/${trackUrn.replace(/:/g, '_')}.mp3`;
  }

  /** Публичный URL трека на CDN */
  getCdnUrl(trackUrn: string, quality: CdnQuality): string {
    return `${this.baseUrl}/${this.trackPath(trackUrn, quality)}`;
  }

  /**
   * Ищет кэшированный трек в БД.
   * Если preferHq — сначала ищет hq, потом sq.
   * Возвращает null если нет записи со status='ok'.
   */
  async findCachedTrack(trackUrn: string, preferHq: boolean): Promise<CdnTrack | null> {
    if (!this.enabled) return null;

    const records = await this.cdnTrackRepo.find({
      where: { trackUrn, status: CdnStatus.OK },
    });

    if (!records.length) return null;

    if (preferHq) {
      return records.find((r) => r.quality === CdnQuality.HQ) ?? records[0];
    }
    return records.find((r) => r.quality === CdnQuality.SQ) ?? records[0];
  }

  /** Получить hqAvailable флаг для trackUrn (из любой записи) */
  async getHqAvailable(trackUrn: string): Promise<boolean | null> {
    const record = await this.cdnTrackRepo.findOne({
      where: { trackUrn },
      select: ['hqAvailable'],
    });
    return record?.hqAvailable ?? null;
  }

  /** Установить hqAvailable флаг на всех записях trackUrn */
  async setHqAvailable(trackUrn: string, available: boolean): Promise<void> {
    await this.cdnTrackRepo.update({ trackUrn }, { hqAvailable: available });
  }

  isTemporarilyUnavailable(): boolean {
    return this.unavailableUntil > Date.now();
  }

  /** Проверяет что CDN реально отдаёт файл. */
  async verifyCdnUrl(url: string): Promise<CdnVerifyResult> {
    if (this.isTemporarilyUnavailable()) {
      this.logger.debug('CDN breaker open, skipping HEAD check');
      return 'unavailable';
    }

    try {
      const { status } = await firstValueFrom(
        this.httpService.head(url, {
          validateStatus: () => true,
          timeout: 3000,
        }),
      );

      if (status >= 200 && status < 300) {
        this.markAvailable();
        return 'ok';
      }

      if (status === 404 || status === 410) {
        this.markAvailable();
        return 'missing';
      }

      this.markUnavailable(`HEAD ${status} ${url}`);
      return 'unavailable';
    } catch (err: any) {
      this.markUnavailable(`HEAD error ${url}: ${err.message}`);
      return 'unavailable';
    }
  }

  /** Пометить запись как error */
  async markError(id: string): Promise<void> {
    await this.cdnTrackRepo.update(id, { status: CdnStatus.ERROR });
  }

  /**
   * Проверяет есть ли pending/error записи для retry.
   * Pending старше uploadTimeoutMs → error.
   * Error записи удаляются для retry.
   */
  async cleanupForRetry(trackUrn: string, quality: CdnQuality): Promise<void> {
    // Pending → error если таймаут
    const pendingRecords = await this.cdnTrackRepo.find({
      where: { trackUrn, quality, status: CdnStatus.PENDING },
    });
    const now = Date.now();
    for (const record of pendingRecords) {
      if (now - record.createdAt.getTime() > this.uploadTimeoutMs) {
        await this.cdnTrackRepo.update(record.id, { status: CdnStatus.ERROR });
        this.logger.warn(`CDN upload timeout for ${trackUrn} (${quality}), marking error`);
      }
    }

    // Удаляем error записи чтобы можно было retry
    await this.cdnTrackRepo.delete({ trackUrn, quality, status: CdnStatus.ERROR });
  }

  /** Есть ли активный pending (не истёкший) для этого трека+качества */
  async hasPending(trackUrn: string, quality: CdnQuality): Promise<boolean> {
    const record = await this.cdnTrackRepo.findOne({
      where: { trackUrn, quality, status: CdnStatus.PENDING },
    });
    if (!record) return false;
    if (Date.now() - record.createdAt.getTime() > this.uploadTimeoutMs) {
      await this.cdnTrackRepo.update(record.id, { status: CdnStatus.ERROR });
      return false;
    }
    return true;
  }

  /**
   * Загружает файл на CDN с трекингом в БД.
   * Принимает путь к tmp-файлу, стримит его при загрузке, удаляет после.
   */
  async uploadWithTracking(
    trackUrn: string,
    quality: CdnQuality,
    filePath: string,
  ): Promise<boolean> {
    if (!this.enabled) return false;
    if (this.isTemporarilyUnavailable()) {
      this.logger.debug(`CDN breaker open, skipping upload for ${trackUrn} (${quality})`);
      await this.cleanupTmpFile(filePath);
      return false;
    }

    const cdnPath = this.trackPath(trackUrn, quality);

    // Проверяем нет ли уже активного pending
    if (await this.hasPending(trackUrn, quality)) {
      this.logger.debug(`CDN upload already pending for ${trackUrn} (${quality}), skipping`);
      await this.cleanupTmpFile(filePath);
      return false;
    }

    // Cleanup error записи для retry
    await this.cleanupForRetry(trackUrn, quality);

    // Проверяем нет ли уже ok записи
    const existing = await this.cdnTrackRepo.findOne({
      where: { trackUrn, quality, status: CdnStatus.OK },
    });
    if (existing) {
      await this.cleanupTmpFile(filePath);
      return true;
    }

    // Создаём pending запись
    const record = this.cdnTrackRepo.create({
      trackUrn,
      quality,
      cdnPath,
      status: CdnStatus.PENDING,
    });
    await this.cdnTrackRepo.save(record);

    try {
      const fileSize = statSync(filePath).size;
      const uploadResult = await this.uploadToCdn(cdnPath, filePath, fileSize);
      if (uploadResult === 'uploaded') {
        await this.cdnTrackRepo.update(record.id, {
          status: CdnStatus.OK,
          cdnPath,
        });
        this.logger.log(`CDN uploaded: ${cdnPath} (${(fileSize / 1024 / 1024).toFixed(1)}MB)`);
        return true;
      }

      if (uploadResult === 'unavailable') {
        await this.cdnTrackRepo.delete(record.id);
        return false;
      }

      await this.cdnTrackRepo.update(record.id, { status: CdnStatus.ERROR });
      return false;
    } catch (err: any) {
      this.logger.warn(`CDN upload error for ${trackUrn}: ${err.message}`);
      await this.cdnTrackRepo.update(record.id, { status: CdnStatus.ERROR });
      return false;
    } finally {
      await this.cleanupTmpFile(filePath);
    }
  }

  /** Двухфазная загрузка на SecureServe CDN (стримит файл, не грузит в память) */
  private async uploadToCdn(
    path: string,
    filePath: string,
    fileSize: number,
  ): Promise<CdnUploadResult> {
    if (this.isTemporarilyUnavailable()) {
      return 'unavailable';
    }

    const uploadToken = `sc-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;

    // Phase 1: Sign upload
    let signRes: AxiosResponse<{ token: string }>;
    try {
      signRes = await firstValueFrom(
        this.httpService.post(
          `${this.baseUrl}/api/sign-upload`,
          {
            token: uploadToken,
            path,
            size: fileSize,
            content_type: 'audio/mpeg',
          },
          {
            headers: {
              Authorization: `Bearer ${this.authToken}`,
              'Content-Type': 'application/json',
            },
            timeout: 10_000,
            validateStatus: () => true,
          },
        ),
      );
    } catch (err: any) {
      this.markUnavailable(`sign-upload error ${path}: ${err.message}`);
      return 'unavailable';
    }

    if (signRes.status !== 200) {
      if (this.isUnavailableStatus(signRes.status)) {
        this.markUnavailable(`sign-upload ${signRes.status} ${path}`);
        return 'unavailable';
      }
      this.markAvailable();
      this.logger.warn(`CDN sign-upload failed: ${signRes.status}`);
      return 'failed';
    }

    // Phase 2: Upload file (стрим из tmp-файла)
    const FormData = (await import('form-data')).default;
    const form = new FormData();
    form.append('token', signRes.data.token);
    form.append('file', createReadStream(filePath), {
      filename: 'track.mp3',
      contentType: 'audio/mpeg',
      knownLength: fileSize,
    });

    let uploadRes: AxiosResponse;
    try {
      uploadRes = await firstValueFrom(
        this.httpService.post(`${this.baseUrl}/api/upload`, form, {
          headers: {
            ...form.getHeaders(),
          },
          timeout: this.uploadTimeoutMs,
          maxBodyLength: Infinity,
          maxContentLength: Infinity,
          validateStatus: () => true,
        }),
      );
    } catch (err: any) {
      this.markUnavailable(`upload error ${path}: ${err.message}`);
      return 'unavailable';
    }

    if (uploadRes.status !== 200) {
      if (this.isUnavailableStatus(uploadRes.status)) {
        this.markUnavailable(`upload ${uploadRes.status} ${path}`);
        return 'unavailable';
      }
      this.markAvailable();
      this.logger.warn(`CDN upload failed: ${uploadRes.status}`);
      return 'failed';
    }

    this.markAvailable();
    return 'uploaded';
  }

  private async cleanupTmpFile(filePath: string): Promise<void> {
    try {
      await unlink(filePath);
    } catch {}
  }

  private isUnavailableStatus(status: number): boolean {
    return (
      status === 401 ||
      status === 403 ||
      status === 408 ||
      status === 425 ||
      status === 429 ||
      status >= 500
    );
  }

  private markAvailable(): void {
    if (this.consecutiveUnavailable > 0 || this.unavailableUntil > 0) {
      this.logger.log('CDN reachable again, closing breaker');
    }
    this.consecutiveUnavailable = 0;
    this.unavailableUntil = 0;
  }

  private markUnavailable(reason: string): void {
    this.consecutiveUnavailable += 1;

    if (this.isTemporarilyUnavailable()) {
      return;
    }

    if (this.consecutiveUnavailable < this.unavailableThreshold) {
      this.logger.warn(
        `CDN unavailable (${this.consecutiveUnavailable}/${this.unavailableThreshold}): ${reason}`,
      );
      return;
    }

    this.unavailableUntil = Date.now() + this.unavailableCooldownMs;
    this.logger.warn(
      `CDN breaker opened for ${this.unavailableCooldownMs}ms after ${this.consecutiveUnavailable} failures: ${reason}`,
    );
  }
}
