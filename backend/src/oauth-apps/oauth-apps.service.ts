import { HttpService } from '@nestjs/axios';
import { Injectable, Logger, NotFoundException, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { firstValueFrom } from 'rxjs';
import { Repository } from 'typeorm';
import { OAuthApp } from './entities/oauth-app.entity.js';

@Injectable()
export class OAuthAppsService implements OnModuleInit {
  private readonly logger = new Logger(OAuthAppsService.name);
  private readonly telegramBotToken: string;
  private readonly telegramChatId: string;

  constructor(
    @InjectRepository(OAuthApp)
    private readonly repo: Repository<OAuthApp>,
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.telegramBotToken = this.configService.get<string>('telegram.botToken') ?? '';
    this.telegramChatId = this.configService.get<string>('telegram.chatId') ?? '';
  }

  async onModuleInit() {
    await this.migrateEnvApp();
    const total = await this.repo.count({ where: { active: true } });
    this.logger.log(`Active OAuth apps: ${total}`);
  }

  /**
   * Если в БД нет ни одной аппки, создаёт одну из env-переменных (обратная совместимость)
   */
  private async migrateEnvApp() {
    const count = await this.repo.count();
    if (count > 0) return;

    const clientId = this.configService.get<string>('soundcloud.clientId');
    const clientSecret = this.configService.get<string>('soundcloud.clientSecret');
    const redirectUri = this.configService.get<string>('soundcloud.redirectUri');

    if (clientId && clientSecret) {
      const app = this.repo.create({
        name: 'default',
        clientId,
        clientSecret,
        redirectUri: redirectUri || 'http://localhost:3000/auth/callback',
        active: true,
      });
      await this.repo.save(app);
      this.logger.log('Migrated env OAuth credentials to oauth_apps table');
    }
  }

  /** Есть ли хотя бы одна активная аппка (без побочных эффектов). */
  async hasActiveApp(): Promise<boolean> {
    const count = await this.repo.count({ where: { active: true } });
    return count > 0;
  }

  /**
   * Атомарно выбирает активную аппку, у которой `lastUsedAt` самый старый
   * (NULL — наивысший приоритет, такие никогда не использовались), и сразу
   * проставляет её `lastUsedAt = now()`. Это размазывает логины по аппкам
   * и помогает обходить per-app rate limit'ы SoundCloud.
   *
   * `FOR UPDATE SKIP LOCKED` гарантирует, что параллельные логины возьмут
   * разные строки, а не одну и ту же.
   */
  async pickLeastRecentlyUsedApp(): Promise<OAuthApp> {
    return this.repo.manager.transaction(async (em) => {
      const app = await em
        .createQueryBuilder(OAuthApp, 'app')
        .where('app.active = :active', { active: true })
        .orderBy('app.lastUsedAt', 'ASC', 'NULLS FIRST')
        .addOrderBy('app.createdAt', 'ASC')
        .setLock('pessimistic_write')
        .setOnLocked('skip_locked')
        .getOne();
      if (!app) {
        throw new NotFoundException('No active OAuth apps available');
      }
      app.lastUsedAt = new Date();
      await em.save(app);
      this.logger.log(`Picked OAuth app "${app.name}" (${app.id}) — lastUsedAt updated`);
      return app;
    });
  }

  /** Получить аппку по ID (для token exchange/refresh) */
  async getById(id: string): Promise<OAuthApp | null> {
    return this.repo.findOne({ where: { id } });
  }

  /** Все аппки */
  async findAll(): Promise<OAuthApp[]> {
    return this.repo.find({ order: { createdAt: 'ASC' } });
  }

  /** Создать новую аппку */
  async create(data: {
    name: string;
    clientId: string;
    clientSecret: string;
    redirectUri: string;
    active?: boolean;
  }): Promise<OAuthApp> {
    const app = this.repo.create({ ...data, active: data.active !== false });
    return this.repo.save(app);
  }

  /** Обновить аппку */
  async update(
    id: string,
    data: Partial<Pick<OAuthApp, 'name' | 'clientId' | 'clientSecret' | 'redirectUri' | 'active'>>,
  ): Promise<OAuthApp> {
    const app = await this.repo.findOne({ where: { id } });
    if (!app) throw new NotFoundException('OAuth app not found');
    Object.assign(app, data);
    return this.repo.save(app);
  }

  /** Удалить аппку */
  async remove(id: string): Promise<void> {
    await this.repo.delete(id);
  }

  // ─── Telegram ────────────────────────────────────────────

  private async sendTelegramAlert(text: string): Promise<void> {
    if (!this.telegramBotToken || !this.telegramChatId) {
      this.logger.warn('Telegram not configured, skipping alert');
      return;
    }

    try {
      await firstValueFrom(
        this.httpService.post(`https://api.telegram.org/bot${this.telegramBotToken}/sendMessage`, {
          chat_id: this.telegramChatId,
          text,
          parse_mode: 'HTML',
        }),
      );
      this.logger.log('Telegram alert sent');
    } catch (err: any) {
      this.logger.error(`Telegram alert failed: ${err.message}`);
    }
  }

  /** Общий алерт (можно использовать для любых уведомлений) */
  async notify(text: string): Promise<void> {
    await this.sendTelegramAlert(text);
  }
}
