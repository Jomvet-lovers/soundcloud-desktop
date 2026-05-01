import { Controller, Get, Headers, Inject, UnauthorizedException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { count, gt } from 'drizzle-orm';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { sessions } from '../db/schema.js';

@ApiTags('admin')
@Controller('admin')
export class AdminStatsController {
  private readonly adminToken: string;

  constructor(
    @Inject(DB) private readonly db: Database,
    configService: ConfigService,
  ) {
    this.adminToken = configService.get<string>('admin.token') ?? '';
  }

  private checkAdmin(token: string | undefined) {
    if (!this.adminToken || token !== this.adminToken) {
      throw new UnauthorizedException('Invalid admin token');
    }
  }

  @Get('stats')
  @ApiOperation({ summary: 'Get active users statistics' })
  @ApiHeader({ name: 'x-admin-token', required: true })
  async getStats(@Headers('x-admin-token') adminToken: string) {
    this.checkAdmin(adminToken);

    const now = new Date();
    const ago24h = new Date(now.getTime() - 24 * 60 * 60 * 1000);
    const ago7d = new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000);
    const ago30d = new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000);

    const countWhere = (since?: Date) =>
      this.db
        .select({ n: count() })
        .from(sessions)
        .where(since ? gt(sessions.updatedAt, since) : undefined)
        .then((r) => r[0]?.n ?? 0);

    const [active24h, active7d, active30d, total] = await Promise.all([
      countWhere(ago24h),
      countWhere(ago7d),
      countWhere(ago30d),
      countWhere(),
    ]);

    return {
      active_24h: active24h,
      active_7d: active7d,
      active_30d: active30d,
      total_sessions: total,
    };
  }
}
