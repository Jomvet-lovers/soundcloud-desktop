import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { InjectRepository } from '@nestjs/typeorm';
import { MoreThan, Repository } from 'typeorm';
import { Subscription } from './entities/subscription.entity.js';

@Injectable()
export class SubscriptionsService implements OnModuleInit {
  private readonly logger = new Logger(SubscriptionsService.name);
  private readonly snapshotDir: string;
  private readonly snapshotFile: string;
  private snapshotTimer: NodeJS.Timeout | null = null;

  constructor(
    @InjectRepository(Subscription)
    private readonly repo: Repository<Subscription>,
    configService: ConfigService,
  ) {
    this.snapshotDir = configService.get<string>('subscriptions.snapshotDir') ?? '/snapshots';
    this.snapshotFile = join(this.snapshotDir, 'subscriptions.json');
  }

  async onModuleInit() {
    await this.restoreFromSnapshot();
    // Export snapshot every 5 minutes
    this.snapshotTimer = setInterval(() => this.exportSnapshot(), 5 * 60 * 1000);
  }

  async isPremium(userUrn: string): Promise<boolean> {
    const now = Math.floor(Date.now() / 1000);
    const sub = await this.repo.findOne({
      where: { userUrn, expDate: MoreThan(now) },
    });
    return !!sub;
  }

  async list(): Promise<Subscription[]> {
    return this.repo.find({ order: { expDate: 'DESC' } });
  }

  async upsert(userUrn: string, expDate: number): Promise<void> {
    await this.repo.upsert({ userUrn, expDate }, ['userUrn']);
    await this.exportSnapshot();
  }

  async remove(userUrn: string): Promise<boolean> {
    const result = await this.repo.delete({ userUrn });
    if (result.affected && result.affected > 0) {
      await this.exportSnapshot();
      return true;
    }
    return false;
  }

  private async exportSnapshot(): Promise<void> {
    try {
      const subs = await this.repo.find();
      mkdirSync(this.snapshotDir, { recursive: true });
      writeFileSync(this.snapshotFile, JSON.stringify(subs, null, 2));
    } catch (err) {
      this.logger.warn(`Snapshot export failed: ${err}`);
    }
  }

  private async restoreFromSnapshot(): Promise<void> {
    try {
      const count = await this.repo.count();
      if (count > 0) {
        this.logger.log(`Subscriptions table has ${count} entries, skipping restore`);
        return;
      }

      if (!existsSync(this.snapshotFile)) {
        this.logger.log('No snapshot file found, starting fresh');
        return;
      }

      const raw = readFileSync(this.snapshotFile, 'utf-8');
      const subs: Subscription[] = JSON.parse(raw);
      if (subs.length === 0) return;

      await this.repo.upsert(subs, ['userUrn']);
      this.logger.log(`Restored ${subs.length} subscriptions from snapshot`);
    } catch (err) {
      this.logger.warn(`Snapshot restore failed: ${err}`);
    }
  }
}
