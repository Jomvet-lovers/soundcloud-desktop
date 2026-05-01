import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { Inject, Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { count, desc, eq, sql } from 'drizzle-orm';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { type Subscription, subscriptions } from '../db/schema.js';

@Injectable()
export class SubscriptionsService implements OnModuleInit {
  private readonly logger = new Logger(SubscriptionsService.name);
  private readonly snapshotDir: string;
  private readonly snapshotFile: string;
  private snapshotTimer: NodeJS.Timeout | null = null;

  constructor(
    @Inject(DB) private readonly db: Database,
    configService: ConfigService,
  ) {
    this.snapshotDir = configService.get<string>('subscriptions.snapshotDir') ?? '/snapshots';
    this.snapshotFile = join(this.snapshotDir, 'subscriptions.json');
  }

  async onModuleInit() {
    await this.restoreFromSnapshot();
    this.snapshotTimer = setInterval(() => this.exportSnapshot(), 5 * 60 * 1000);
  }

  async isPremium(userUrn: string): Promise<boolean> {
    const now = Math.floor(Date.now() / 1000);
    const row = await this.db.query.subscriptions.findFirst({
      where: eq(subscriptions.userUrn, userUrn),
    });
    return !!row && row.expDate > now;
  }

  async list(): Promise<Subscription[]> {
    return this.db.select().from(subscriptions).orderBy(desc(subscriptions.expDate));
  }

  async upsert(userUrn: string, expDate: number): Promise<void> {
    await this.db
      .insert(subscriptions)
      .values({ userUrn, expDate })
      .onConflictDoUpdate({ target: subscriptions.userUrn, set: { expDate } });
    await this.exportSnapshot();
  }

  async remove(userUrn: string): Promise<boolean> {
    const deleted = await this.db
      .delete(subscriptions)
      .where(eq(subscriptions.userUrn, userUrn))
      .returning({ urn: subscriptions.userUrn });
    if (deleted.length > 0) {
      await this.exportSnapshot();
      return true;
    }
    return false;
  }

  private async exportSnapshot(): Promise<void> {
    try {
      const subs = await this.db.select().from(subscriptions);
      mkdirSync(this.snapshotDir, { recursive: true });
      writeFileSync(this.snapshotFile, JSON.stringify(subs, null, 2));
    } catch (err) {
      this.logger.warn(`Snapshot export failed: ${err}`);
    }
  }

  private async restoreFromSnapshot(): Promise<void> {
    try {
      const total = await this.db.select({ n: count() }).from(subscriptions);
      if ((total[0]?.n ?? 0) > 0) {
        this.logger.log(`Subscriptions table has ${total[0].n} entries, skipping restore`);
        return;
      }
      if (!existsSync(this.snapshotFile)) {
        this.logger.log('No snapshot file found, starting fresh');
        return;
      }
      const raw = readFileSync(this.snapshotFile, 'utf-8');
      const subs: Subscription[] = JSON.parse(raw);
      if (subs.length === 0) return;

      await this.db
        .insert(subscriptions)
        .values(subs)
        .onConflictDoUpdate({
          target: subscriptions.userUrn,
          set: { expDate: sql`excluded.exp_date` },
        });
      this.logger.log(`Restored ${subs.length} subscriptions from snapshot`);
    } catch (err) {
      this.logger.warn(`Snapshot restore failed: ${err}`);
    }
  }
}
