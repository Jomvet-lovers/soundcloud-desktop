import { Controller, Get, Headers, UnauthorizedException } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { InjectRepository } from '@nestjs/typeorm';
import { MoreThan, Repository } from 'typeorm';
import { Session } from '../auth/entities/session.entity.js';

@ApiTags('admin')
@Controller('admin')
export class AdminStatsController {
  private readonly adminToken: string;

  constructor(
    @InjectRepository(Session)
    private readonly sessionRepo: Repository<Session>,
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

    const [active24h, active7d, active30d, total] = await Promise.all([
      this.sessionRepo.count({ where: { updatedAt: MoreThan(ago24h) } }),
      this.sessionRepo.count({ where: { updatedAt: MoreThan(ago7d) } }),
      this.sessionRepo.count({ where: { updatedAt: MoreThan(ago30d) } }),
      this.sessionRepo.count(),
    ]);

    return {
      active_24h: active24h,
      active_7d: active7d,
      active_30d: active30d,
      total_sessions: total,
    };
  }
}
