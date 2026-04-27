import { Injectable } from '@nestjs/common';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { PendingActionsService } from '../pending-actions/pending-actions.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import type {
  ScComment,
  ScPaginatedResponse,
  ScStreams,
  ScTrack,
  ScUser,
} from '../soundcloud/soundcloud.types.js';

@Injectable()
export class TracksService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly localLikes: LocalLikesService,
    private readonly pendingActions: PendingActionsService,
  ) {}

  private async applyLocalLikeFlags(sessionId: string, tracks: ScTrack[]): Promise<ScTrack[]> {
    const urns = tracks.map((track) => track.urn).filter(Boolean);
    const likedUrns = await this.localLikes.getLikedTrackIds(sessionId, urns);
    if (likedUrns.size === 0) {
      return tracks;
    }

    return tracks.map((track) =>
      likedUrns.has(track.urn) ? { ...track, user_favorite: true } : track,
    );
  }

  async search(
    token: string,
    sessionId: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    const response = await this.sc.apiGet<ScPaginatedResponse<ScTrack>>('/tracks', token, params);
    response.collection = await this.applyLocalLikeFlags(sessionId, response.collection ?? []);
    return response;
  }

  async getById(
    token: string,
    sessionId: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScTrack> {
    const track = await this.sc.apiGet<ScTrack>(`/tracks/${trackUrn}`, token, params);
    const [annotated] = await this.applyLocalLikeFlags(sessionId, [track]);
    return annotated;
  }

  update(token: string, trackUrn: string, body: unknown): Promise<ScTrack> {
    return this.sc.apiPut(`/tracks/${trackUrn}`, token, body);
  }

  delete(token: string, trackUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/tracks/${trackUrn}`, token);
  }

  getStreams(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScStreams> {
    return this.sc.apiGet(`/tracks/${trackUrn}/streams`, token, params);
  }

  getComments(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScComment>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/comments`, token, params);
  }

  async createComment(
    token: string,
    sessionId: string,
    trackUrn: string,
    body: { comment: { body: string; timestamp?: number } },
  ): Promise<unknown> {
    try {
      return await this.sc.apiPost<ScComment>(`/tracks/${trackUrn}/comments`, token, body);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(
          sessionId,
          'comment',
          trackUrn,
          body as unknown as Record<string, unknown>,
        );
        return { queued: true, actionType: 'comment', targetUrn: trackUrn };
      }
      throw error;
    }
  }

  getFavoriters(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/favoriters`, token, params);
  }

  getReposters(
    token: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet(`/tracks/${trackUrn}/reposters`, token, params);
  }

  async getRelated(
    token: string,
    sessionId: string,
    trackUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    const response = await this.sc.apiGet<ScPaginatedResponse<ScTrack>>(
      `/tracks/${trackUrn}/related`,
      token,
      params,
    );
    response.collection = await this.applyLocalLikeFlags(sessionId, response.collection ?? []);
    return response;
  }
}
