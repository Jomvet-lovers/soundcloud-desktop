import { forwardRef, Inject, Injectable, UnauthorizedException } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { In, LessThan, Repository } from 'typeorm';
import { AuthService } from '../auth/auth.service.js';
import { normalizeScTrackId } from '../common/sc-ids.js';
import { EventsService } from '../events/events.service.js';
import { DislikedTrack } from './entities/disliked-track.entity.js';

@Injectable()
export class DislikesService {
  constructor(
    @InjectRepository(DislikedTrack)
    private readonly repo: Repository<DislikedTrack>,
    private readonly authService: AuthService,
    @Inject(forwardRef(() => EventsService))
    private readonly events: EventsService,
  ) {}

  private async getScUserId(sessionId: string): Promise<string> {
    const session = await this.authService.getSession(sessionId);
    if (!session?.soundcloudUserId) {
      throw new UnauthorizedException('User not found in session');
    }
    return session.soundcloudUserId;
  }

  async add(
    sessionId: string,
    scTrackId: string,
    trackData?: Record<string, unknown>,
  ): Promise<{ status: string }> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return { status: 'invalid' };
    const scUserId = await this.getScUserId(sessionId);
    const existing = await this.repo.findOne({ where: { scUserId, scTrackId: id } });
    if (!existing) {
      await this.repo.save(
        this.repo.create({ scUserId, scTrackId: id, trackData: trackData ?? null }),
      );
      await this.events.record(scUserId, id, 'dislike');
    }
    return { status: 'ok' };
  }

  async remove(sessionId: string, scTrackId: string): Promise<{ status: string }> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return { status: 'invalid' };
    const scUserId = await this.getScUserId(sessionId);
    await this.repo.delete({ scUserId, scTrackId: id });
    return { status: 'removed' };
  }

  async isDisliked(sessionId: string, scTrackId: string): Promise<boolean> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return false;
    const scUserId = await this.getScUserId(sessionId);
    return !!(await this.repo.findOne({ where: { scUserId, scTrackId: id } }));
  }

  async isDislikedByUserId(scUserId: string, scTrackId: string): Promise<boolean> {
    const id = normalizeScTrackId(scTrackId);
    if (!id) return false;
    return !!(await this.repo.findOne({ where: { scUserId, scTrackId: id } }));
  }

  async getDislikedTrackIds(sessionId: string, scTrackIds: string[]): Promise<Set<string>> {
    if (scTrackIds.length === 0) return new Set();
    const normalized = scTrackIds
      .map((id) => normalizeScTrackId(id))
      .filter((id): id is string => !!id);
    if (normalized.length === 0) return new Set();
    const scUserId = await this.getScUserId(sessionId);
    const items = await this.repo.find({
      select: { scTrackId: true },
      where: { scUserId, scTrackId: In(normalized) },
    });
    return new Set(items.map((item) => item.scTrackId));
  }

  async listIdsByUserId(scUserId: string, limit = 200): Promise<string[]> {
    const items = await this.repo.find({
      select: { scTrackId: true },
      where: { scUserId },
      order: { createdAt: 'DESC' },
      take: limit,
    });
    return items.map((item) => item.scTrackId);
  }

  async findAll(
    sessionId: string,
    limit: number,
    cursor?: string,
  ): Promise<{ collection: Record<string, unknown>[]; next_href: string | null }> {
    const scUserId = await this.getScUserId(sessionId);
    const where: Record<string, unknown> = { scUserId };
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
      collection: collection
        .map((item) => item.trackData)
        .filter((d): d is Record<string, unknown> => d !== null),
      next_href: hasMore
        ? `?limit=${limit}&cursor=${collection[collection.length - 1].createdAt.toISOString()}`
        : null,
    };
  }
}
