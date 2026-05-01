import { randomBytes } from 'node:crypto';
import { Inject, Injectable, Logger, OnApplicationBootstrap, OnModuleDestroy } from '@nestjs/common';
import { SchedulerRegistry } from '@nestjs/schedule';
import { Redis } from 'ioredis';
import { REDIS_CLIENT } from '../../cache/cache.constants.js';

const LOCK_KEY = 'scd:cron:leader';
const LOCK_TTL_MS = 30_000;
const RENEW_INTERVAL_MS = 10_000;

@Injectable()
export class CronLeaderService implements OnApplicationBootstrap, OnModuleDestroy {
  private readonly logger = new Logger(CronLeaderService.name);
  private readonly token = `${process.env.HOSTNAME ?? 'host'}-${process.env.BACKEND_INDEX ?? '?'}-${process.pid}-${randomBytes(4).toString('hex')}`;
  private leader = false;
  private initialized = false;
  private interval?: NodeJS.Timeout;

  constructor(
    @Inject(REDIS_CLIENT) private readonly redis: Redis,
    private readonly registry: SchedulerRegistry,
  ) {}

  isLeader(): boolean {
    return this.leader;
  }

  async onApplicationBootstrap(): Promise<void> {
    await this.tick();
    this.interval = setInterval(() => {
      void this.tick();
    }, RENEW_INTERVAL_MS);
  }

  async onModuleDestroy(): Promise<void> {
    if (this.interval) clearInterval(this.interval);
    if (!this.leader) return;
    try {
      const owner = await this.redis.get(LOCK_KEY);
      if (owner === this.token) await this.redis.del(LOCK_KEY);
    } catch {}
  }

  private async tick(): Promise<void> {
    try {
      const acquired = await this.redis.set(LOCK_KEY, this.token, 'PX', LOCK_TTL_MS, 'NX');
      if (acquired === 'OK') {
        this.becomeLeader();
        return;
      }
      const owner = await this.redis.get(LOCK_KEY);
      if (owner === this.token) {
        await this.redis.set(LOCK_KEY, this.token, 'PX', LOCK_TTL_MS, 'XX');
        this.becomeLeader();
      } else {
        this.becomeFollower();
      }
    } catch (e) {
      this.logger.warn(`leader tick failed: ${(e as Error).message}`);
      this.becomeFollower();
    }
  }

  private becomeLeader(): void {
    if (this.leader && this.initialized) return;
    this.leader = true;
    this.initialized = true;
    this.logger.log(`became cron leader (${this.token})`);
    for (const [, job] of this.registry.getCronJobs()) {
      try {
        job.start();
      } catch {}
    }
  }

  private becomeFollower(): void {
    if (!this.leader && this.initialized) return;
    this.leader = false;
    this.initialized = true;
    this.logger.log('not cron leader');
    for (const [, job] of this.registry.getCronJobs()) {
      try {
        job.stop();
      } catch {}
    }
  }
}
