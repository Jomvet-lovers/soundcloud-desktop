import { HttpService } from '@nestjs/axios';
import { Inject, Injectable, Logger, NotFoundException, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { asc, count, eq, sql } from 'drizzle-orm';
import { firstValueFrom } from 'rxjs';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { type OAuthApp, oauthApps } from '../db/schema.js';

@Injectable()
export class OAuthAppsService implements OnModuleInit {
  private readonly logger = new Logger(OAuthAppsService.name);
  private readonly telegramBotToken: string;
  private readonly telegramChatId: string;

  constructor(
    @Inject(DB) private readonly db: Database,
    private readonly httpService: HttpService,
    private readonly configService: ConfigService,
  ) {
    this.telegramBotToken = this.configService.get<string>('telegram.botToken') ?? '';
    this.telegramChatId = this.configService.get<string>('telegram.chatId') ?? '';
  }

  async onModuleInit() {
    await this.migrateEnvApp();
    const total = await this.countActive();
    this.logger.log(`Active OAuth apps: ${total}`);
  }

  private async migrateEnvApp() {
    const total = await this.db.select({ n: count() }).from(oauthApps);
    if ((total[0]?.n ?? 0) > 0) return;

    const clientId = this.configService.get<string>('soundcloud.clientId');
    const clientSecret = this.configService.get<string>('soundcloud.clientSecret');
    const redirectUri = this.configService.get<string>('soundcloud.redirectUri');

    if (clientId && clientSecret) {
      await this.db.insert(oauthApps).values({
        name: 'default',
        clientId,
        clientSecret,
        redirectUri: redirectUri || 'http://localhost:3000/auth/callback',
        active: true,
      });
      this.logger.log('Migrated env OAuth credentials to oauth_apps table');
    }
  }

  private async countActive(): Promise<number> {
    const rows = await this.db
      .select({ n: count() })
      .from(oauthApps)
      .where(eq(oauthApps.active, true));
    return rows[0]?.n ?? 0;
  }

  async hasActiveApp(): Promise<boolean> {
    return (await this.countActive()) > 0;
  }

  async pickLeastRecentlyUsedApp(): Promise<OAuthApp> {
    return this.db.transaction(async (tx) => {
      const [app] = await tx
        .select()
        .from(oauthApps)
        .where(eq(oauthApps.active, true))
        .orderBy(sql`${oauthApps.lastUsedAt} ASC NULLS FIRST`, asc(oauthApps.createdAt))
        .limit(1)
        .for('update', { skipLocked: true });

      if (!app) {
        throw new NotFoundException('No active OAuth apps available');
      }

      const [updated] = await tx
        .update(oauthApps)
        .set({ lastUsedAt: new Date() })
        .where(eq(oauthApps.id, app.id))
        .returning();

      this.logger.log(`Picked OAuth app "${updated.name}" (${updated.id}) — lastUsedAt updated`);
      return updated;
    });
  }

  async getById(id: string): Promise<OAuthApp | null> {
    const row = await this.db.query.oauthApps.findFirst({ where: eq(oauthApps.id, id) });
    return row ?? null;
  }

  async findAll(): Promise<OAuthApp[]> {
    return this.db.select().from(oauthApps).orderBy(asc(oauthApps.createdAt));
  }

  async create(data: {
    name: string;
    clientId: string;
    clientSecret: string;
    redirectUri: string;
    active?: boolean;
  }): Promise<OAuthApp> {
    const [row] = await this.db
      .insert(oauthApps)
      .values({ ...data, active: data.active !== false })
      .returning();
    return row;
  }

  async update(
    id: string,
    data: Partial<Pick<OAuthApp, 'name' | 'clientId' | 'clientSecret' | 'redirectUri' | 'active'>>,
  ): Promise<OAuthApp> {
    const [row] = await this.db
      .update(oauthApps)
      .set(data)
      .where(eq(oauthApps.id, id))
      .returning();
    if (!row) throw new NotFoundException('OAuth app not found');
    return row;
  }

  async remove(id: string): Promise<void> {
    await this.db.delete(oauthApps).where(eq(oauthApps.id, id));
  }

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

  async notify(text: string): Promise<void> {
    await this.sendTelegramAlert(text);
  }
}
