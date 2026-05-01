import { Inject, Injectable, Logger } from '@nestjs/common';
import { desc } from 'drizzle-orm';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { sessions } from '../db/schema.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';

@Injectable()
export class ResolveService {
  private readonly logger = new Logger(ResolveService.name);

  constructor(
    private readonly sc: SoundcloudService,
    @Inject(DB) private readonly db: Database,
  ) {}

  async resolve(token: string, url: string): Promise<unknown> {
    return this.sc.apiGet('/resolve', token, { url });
  }

  async resolveWithRandomToken(url: string): Promise<unknown> {
    const rows = await this.db
      .select({ accessToken: sessions.accessToken })
      .from(sessions)
      .orderBy(desc(sessions.createdAt));

    for (const session of rows) {
      if (!session.accessToken) continue;
      try {
        return await this.sc.apiGet('/resolve', session.accessToken, { url });
      } catch (_err) {
        this.logger.debug(`Token failed for resolve, trying next...`);
      }
    }

    throw new Error('No valid session token available for resolve');
  }
}
