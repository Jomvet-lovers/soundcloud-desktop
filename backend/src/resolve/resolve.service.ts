import { Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { Session } from '../auth/entities/session.entity.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';

@Injectable()
export class ResolveService {
  private readonly logger = new Logger(ResolveService.name);

  constructor(
    private readonly sc: SoundcloudService,
    @InjectRepository(Session)
    private readonly sessionRepo: Repository<Session>,
  ) {}

  resolve(token: string, url: string): Promise<unknown> {
    return this.sc.apiGet('/resolve', token, { url });
  }

  async resolveWithRandomToken(url: string): Promise<unknown> {
    const sessions = await this.sessionRepo.find({
      select: ['accessToken'],
      where: {},
      order: { createdAt: 'DESC' },
    });

    for (const session of sessions) {
      if (!session.accessToken) continue;
      try {
        return await this.sc.apiGet('/resolve', session.accessToken, { url });
      } catch (err) {
        this.logger.debug(`Token failed for resolve, trying next...`);
      }
    }

    throw new Error('No valid session token available for resolve');
  }
}
