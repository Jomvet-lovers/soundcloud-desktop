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

  /** In-memory кэш активных аппок, обновляется при любых мутациях */
  private activeApps: OAuthApp[] = [];

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
    await this.reloadActiveApps();
    await this.migrateEnvApp();
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
      await this.reloadActiveApps();
    }
  }

  private async reloadActiveApps() {
    this.activeApps = await this.repo.find({
      where: { active: true },
      order: { createdAt: 'ASC' },
    });
    this.logger.log(`Active OAuth apps: ${this.activeApps.length}`);
  }

  /** Рандомно выбирает активную аппку для нового логина */
  pickRandomApp(): OAuthApp {
    if (this.activeApps.length === 0) {
      throw new NotFoundException('No active OAuth apps available');
    }
    const index = Math.floor(Math.random() * this.activeApps.length);
    return this.activeApps[index];
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
  }): Promise<OAuthApp> {
    const app = this.repo.create({ ...data, active: true });
    const saved = await this.repo.save(app);
    await this.reloadActiveApps();
    return saved;
  }

  /** Обновить аппку */
  async update(
    id: string,
    data: Partial<Pick<OAuthApp, 'name' | 'clientId' | 'clientSecret' | 'redirectUri' | 'active'>>,
  ): Promise<OAuthApp> {
    const app = await this.repo.findOne({ where: { id } });
    if (!app) throw new NotFoundException('OAuth app not found');
    Object.assign(app, data);
    const saved = await this.repo.save(app);
    await this.reloadActiveApps();
    return saved;
  }

  /** Удалить аппку */
  async remove(id: string): Promise<void> {
    await this.repo.delete(id);
    await this.reloadActiveApps();
  }

  // ─── Telegram ────────────────────────────────────────────

  private async sendTelegramAlert(text: string): Promise<void> {
    if (!this.telegramBotToken || !this.telegramChatId) {
      this.logger.warn('Telegram not configured, skipping alert');
      return;
    }

    try {
      await firstValueFrom(
        this.httpService.post(
          `https://api.telegram.org/bot${this.telegramBotToken}/sendMessage`,
          {
            chat_id: this.telegramChatId,
            text,
            parse_mode: 'HTML',
          },
        ),
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
