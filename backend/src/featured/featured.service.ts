import { Injectable, Logger } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { AuthService } from '../auth/auth.service.js';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import type { ScPlaylist, ScTrack, ScUser } from '../soundcloud/soundcloud.types.js';
import { FeaturedItem, type FeaturedItemType } from './entities/featured-item.entity.js';

export interface FeaturedResult {
  type: FeaturedItemType;
  data: ScTrack | ScPlaylist | ScUser;
}

@Injectable()
export class FeaturedService {
  private readonly logger = new Logger(FeaturedService.name);

  constructor(
    @InjectRepository(FeaturedItem)
    private readonly repo: Repository<FeaturedItem>,
    private readonly sc: SoundcloudService,
    private readonly authService: AuthService,
    private readonly localLikes: LocalLikesService,
  ) {}

  // ─── Admin CRUD ──────────────────────────────────────────

  findAll(): Promise<FeaturedItem[]> {
    return this.repo.find({ order: { createdAt: 'DESC' } });
  }

  create(data: { type: FeaturedItemType; scUrn: string; weight?: number; active?: boolean }) {
    const item = this.repo.create(data);
    return this.repo.save(item);
  }

  async update(
    id: string,
    data: Partial<Pick<FeaturedItem, 'type' | 'scUrn' | 'weight' | 'active'>>,
  ) {
    await this.repo.update(id, data);
    return this.repo.findOneByOrFail({ id });
  }

  async remove(id: string) {
    await this.repo.delete(id);
  }

  // ─── Public pick ─────────────────────────────────────────

  async pick(sessionId: string): Promise<FeaturedResult | null> {
    const items = await this.repo.find({ where: { active: true } });
    if (items.length === 0) return null;

    const picked = weightedRandom(items);
    const token = await this.authService.getValidAccessToken(sessionId);

    try {
      return await this.resolve(picked, token, sessionId);
    } catch (e) {
      this.logger.warn(`Failed to resolve featured ${picked.type} ${picked.scUrn}: ${e}`);
      return null;
    }
  }

  private async resolve(
    item: FeaturedItem,
    token: string,
    sessionId: string,
  ): Promise<FeaturedResult> {
    switch (item.type) {
      case 'track': {
        const track = await this.sc.apiGet<ScTrack>(`/tracks/${item.scUrn}`, token);
        const likedUrns = await this.localLikes.getLikedTrackIds(sessionId, [track.urn]);
        if (likedUrns.has(track.urn)) {
          (track as ScTrack & { user_favorite?: boolean }).user_favorite = true;
        }
        return { type: 'track', data: track };
      }
      case 'playlist': {
        const playlist = await this.sc.apiGet<ScPlaylist>(`/playlists/${item.scUrn}`, token);
        return { type: 'playlist', data: playlist };
      }
      case 'user': {
        const user = await this.sc.apiGet<ScUser>(`/users/${item.scUrn}`, token);
        return { type: 'user', data: user };
      }
    }
  }
}

function weightedRandom(items: FeaturedItem[]): FeaturedItem {
  const totalWeight = items.reduce((sum, i) => sum + i.weight, 0);
  let rand = Math.random() * totalWeight;
  for (const item of items) {
    rand -= item.weight;
    if (rand <= 0) return item;
  }
  return items[items.length - 1];
}
