import { createHash } from 'node:crypto';
import { Inject, Injectable } from '@nestjs/common';
import { QdrantClient } from '@qdrant/js-client-rest';
import { normalizeScTrackId } from '../common/sc-ids.js';

const EVENT_WEIGHTS: Record<string, number> = {
  like: 1.0,
  local_like: 1.0,
  playlist_add: 0.9,
  full_play: 0.3,
  skip: -0.5,
  dislike: -1.0,
};

const EMA_ALPHA = 0.15;

@Injectable()
export class UserTasteService {
  constructor(
    @Inject('QDRANT_CLIENT')
    private readonly qdrant: QdrantClient,
  ) {}

  /**
   * @returns true if at least one branch (mert or lyrics) successfully applied EMA;
   *          false if the track had no vector in either collection (not indexed yet).
   */
  async onUserEvent(scUserId: string, scTrackId: string, eventType: string): Promise<boolean> {
    const weight = EVENT_WEIGHTS[eventType];
    if (!weight) return false;

    const normalized = normalizeScTrackId(scTrackId);
    if (!normalized) return false;
    const trackPointId = Number.parseInt(normalized, 10);
    if (Number.isNaN(trackPointId)) return false;

    const [mertApplied, clapApplied, lyricsApplied] = await Promise.all([
      this.updateBranch('tracks_mert', 'user_taste_mert', trackPointId, scUserId, weight),
      this.updateBranch('tracks_clap', 'user_taste_clap', trackPointId, scUserId, weight),
      this.updateBranch('tracks_lyrics', 'user_taste_lyrics', trackPointId, scUserId, weight),
    ]);
    return mertApplied || clapApplied || lyricsApplied;
  }

  private async updateBranch(
    trackCollection: string,
    tasteCollection: string,
    trackPointId: number,
    scUserId: string,
    weight: number,
  ): Promise<boolean> {
    let trackVec: number[];
    try {
      const trackPoints = await this.qdrant.retrieve(trackCollection, {
        ids: [trackPointId],
        with_vector: true,
      });
      if (!trackPoints.length || !trackPoints[0].vector) return false;
      trackVec = trackPoints[0].vector as number[];
    } catch {
      return false;
    }

    const userId = this.userIdToQdrantId(scUserId);

    const profilePoints = await this.qdrant.retrieve(tasteCollection, {
      ids: [userId],
      with_vector: true,
      with_payload: true,
    });

    let newVec: number[];
    let eventCount = 1;

    if (profilePoints.length && profilePoints[0].vector) {
      const currentVec = profilePoints[0].vector as number[];
      eventCount = ((profilePoints[0].payload?.event_count as number) ?? 0) + 1;
      newVec = currentVec.map((v, i) => (1 - EMA_ALPHA) * v + EMA_ALPHA * weight * trackVec[i]);
    } else {
      newVec = trackVec.map((v) => v * Math.sign(weight));
    }

    const norm = Math.sqrt(newVec.reduce((s, v) => s + v * v, 0));
    if (norm > 0) {
      newVec = newVec.map((v) => v / norm);
    }

    await this.qdrant.upsert(tasteCollection, {
      points: [
        {
          id: userId,
          vector: newVec,
          payload: {
            sc_user_id: scUserId,
            event_count: eventCount,
            updated_at: Date.now(),
          },
        },
      ],
    });
    return true;
  }

  private userIdToQdrantId(userId: string): number {
    const hash = createHash('sha256').update(userId).digest();
    return Number(hash.readBigUInt64BE(0) % BigInt(Number.MAX_SAFE_INTEGER));
  }
}
