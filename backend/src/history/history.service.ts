import { Injectable } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { MoreThan, Repository } from 'typeorm';
import { AuthService } from '../auth/auth.service.js';
import { ListeningHistory } from './entities/listening-history.entity.js';

@Injectable()
export class HistoryService {
  constructor(
    @InjectRepository(ListeningHistory)
    private readonly repo: Repository<ListeningHistory>,
    private readonly authService: AuthService,
  ) {}

  private async getScUserId(sessionId: string): Promise<string> {
    const session = await this.authService.getSession(sessionId);
    if (!session?.soundcloudUserId) {
      throw new Error('User not found in session');
    }
    return session.soundcloudUserId;
  }

  async record(
    sessionId: string,
    data: {
      scTrackId: string;
      title: string;
      artistName: string;
      artworkUrl?: string;
      duration: number;
    },
  ): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);

    // Dedup: same track <60s ago → skip
    const recent = await this.repo.findOne({
      where: {
        soundcloudUserId,
        scTrackId: data.scTrackId,
        playedAt: MoreThan(new Date(Date.now() - 60_000)),
      },
      order: { playedAt: 'DESC' },
    });
    if (recent) return;

    const entry = this.repo.create({
      soundcloudUserId,
      ...data,
    });
    await this.repo.save(entry);
  }

  async findAll(
    sessionId: string,
    limit: number,
    offset: number,
  ): Promise<{ collection: ListeningHistory[]; total: number }> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    const [collection, total] = await this.repo.findAndCount({
      where: { soundcloudUserId },
      order: { playedAt: 'DESC' },
      take: limit,
      skip: offset,
    });
    return { collection, total };
  }

  async clear(sessionId: string): Promise<void> {
    const soundcloudUserId = await this.getScUserId(sessionId);
    await this.repo.delete({ soundcloudUserId });
  }
}
