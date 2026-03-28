import { HttpService } from '@nestjs/axios';
import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { firstValueFrom } from 'rxjs';
import { Repository } from 'typeorm';
import { CdnQuality, CdnStatus, CdnTrack } from './entities/cdn-track.entity.js';

@Injectable()
export class CdnService implements OnModuleInit {
  private readonly logger = new Logger(CdnService.name);
  private readonly baseUrl: string;
  private readonly authToken: string;
  private readonly uploadTimeoutMs: number;

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
    this.uploadTimeoutMs = this.configService.get<number>('cdn.uploadTimeoutMs') ?? 300_000;
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

  /** Проверяет что CDN реально отдаёт файл (HEAD, 2xx) */
  async verifyCdnUrl(url: string): Promise<boolean> {
    try {
      const { status } = await firstValueFrom(
        this.httpService.head(url, {
          validateStatus: () => true,
          timeout: 3000,
        }),
      );
      return status >= 200 && status < 300;
    } catch {
      return false;
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
   * Загружает буфер на CDN с трекингом в БД.
   * Создаёт pending запись, грузит, ставит ok/error.
   */
  async uploadWithTracking(
    trackUrn: string,
    quality: CdnQuality,
    audioBuffer: Buffer,
  ): Promise<boolean> {
    if (!this.enabled) return false;

    const cdnPath = this.trackPath(trackUrn, quality);

    // Проверяем нет ли уже активного pending
    if (await this.hasPending(trackUrn, quality)) {
      this.logger.debug(`CDN upload already pending for ${trackUrn} (${quality}), skipping`);
      return false;
    }

    // Cleanup error записи для retry
    await this.cleanupForRetry(trackUrn, quality);

    // Проверяем нет ли уже ok записи
    const existing = await this.cdnTrackRepo.findOne({
      where: { trackUrn, quality, status: CdnStatus.OK },
    });
    if (existing) return true;

    // Создаём pending запись
    const record = this.cdnTrackRepo.create({
      trackUrn,
      quality,
      cdnPath,
      status: CdnStatus.PENDING,
    });
    await this.cdnTrackRepo.save(record);

    try {
      const success = await this.uploadToCdn(cdnPath, audioBuffer);
      if (success) {
        await this.cdnTrackRepo.update(record.id, {
          status: CdnStatus.OK,
          cdnPath,
        });
        this.logger.log(
          `CDN uploaded: ${cdnPath} (${(audioBuffer.length / 1024 / 1024).toFixed(1)}MB)`,
        );
        return true;
      }
      await this.cdnTrackRepo.update(record.id, { status: CdnStatus.ERROR });
      return false;
    } catch (err: any) {
      this.logger.warn(`CDN upload error for ${trackUrn}: ${err.message}`);
      await this.cdnTrackRepo.update(record.id, { status: CdnStatus.ERROR });
      return false;
    }
  }

  /** Двухфазная загрузка на SecureServe CDN */
  private async uploadToCdn(path: string, audioBuffer: Buffer): Promise<boolean> {
    const uploadToken = `sc-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;

    // Phase 1: Sign upload
    const signRes = await firstValueFrom(
      this.httpService.post(
        `${this.baseUrl}/api/sign-upload`,
        {
          token: uploadToken,
          path,
          size: audioBuffer.length,
          content_type: 'audio/mpeg',
        },
        {
          headers: {
            Authorization: `Bearer ${this.authToken}`,
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
      this.httpService.post(`${this.baseUrl}/api/upload`, form, {
        headers: {
          ...form.getHeaders(),
        },
        timeout: this.uploadTimeoutMs,
        maxBodyLength: Infinity,
      }),
    );

    if (uploadRes.status !== 200) {
      this.logger.warn(`CDN upload failed: ${uploadRes.status}`);
      return false;
    }

    return true;
  }
}
