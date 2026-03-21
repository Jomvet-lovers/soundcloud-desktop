import { Injectable } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { LessThan, Repository } from 'typeorm';
import { AuthService } from '../auth/auth.service.js';
import { LocalLike } from './entities/local-like.entity.js';

@Injectable()
export class LocalLikesService {
  constructor(
    @InjectRepository(LocalLike)
    private readonly repo: Repository<LocalLike>,
    private readonly authService: AuthService,
  ) {}

  private async getScUserId(sessionId: string): Promise<string> {
    const session = await this.authService.getSession(sessionId);
    if (!session?.soundcloudUserId) {
      throw new Error('User not found in session');
    }
    return session.soundcloudUserId;
  }

  async add(
    sessionId: string,
    scTrackId: string,
    trackData: Record<string, unknown>,
  ): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    const existing = await this.repo.findOne({ where: { soundcloudUserId, scTrackId } });
    if (existing) return;
    const entity = this.repo.create({ soundcloudUserId, scTrackId, trackData });
    await this.repo.save(entity);
  }

  async remove(sessionId: string, scTrackId: string): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    await this.repo.delete({ soundcloudUserId, scTrackId });
  }

  async findAll(
    sessionId: string,
    limit: number,
    cursor?: string,
  ): Promise<{ collection: Record<string, unknown>[]; next_href: string | null }> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    const where: Record<string, unknown> = { soundcloudUserId };
    if (cursor) {
      where.createdAt = LessThan(new Date(cursor));
    }

    const items = await this.repo.find({
      where,
      order: { createdAt: 'DESC' },
      take: limit + 1,
    });

    const hasMore = items.length > limit;
    const collection = items.slice(0, limit);

    return {
      collection: collection.map((item) => item.trackData),
      next_href: hasMore
        ? `?limit=${limit}&cursor=${collection[collection.length - 1].createdAt.toISOString()}`
        : null,
    };
  }

  async isLiked(sessionId: string, scTrackId: string): Promise<boolean> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    return !!(await this.repo.findOne({ where: { soundcloudUserId, scTrackId } }));
  }
}
