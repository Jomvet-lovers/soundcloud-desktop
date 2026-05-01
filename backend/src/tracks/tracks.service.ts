import { Injectable } from '@nestjs/common';
import { buildListCacheKey, type ListPageResult } from '../cache/list-cache.service.js';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { PendingActionsService } from '../pending-actions/pending-actions.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import type { ScComment, ScStreams, ScTrack, ScUser } from '../soundcloud/soundcloud.types.js';
import { SoundcloudListService } from '../soundcloud/soundcloud-list.service.js';

const TTL_SEARCH = 300;
const TTL_RELATED = 86400;
const TTL_COMMENTS = 600;
const TTL_FAVORITERS = 600;
const TTL_REPOSTERS = 600;

interface PageInput {
  page: number;
  limit: number;
}

@Injectable()
export class TracksService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly scList: SoundcloudListService,
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
    input: PageInput,
    extra: Record<string, unknown>,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: buildListCacheKey('tracks-search', extra),
      ttl: TTL_SEARCH,
      page: input.page,
      limit: input.limit,
      path: '/tracks',
      token,
      extraParams: extra,
    });
    result.collection = await this.applyLocalLikeFlags(sessionId, result.collection);
    return result;
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
    input: PageInput,
  ): Promise<ListPageResult<ScComment>> {
    return this.scList.getPage<ScComment>({
      cacheKey: `track-comments:${trackUrn}`,
      ttl: TTL_COMMENTS,
      page: input.page,
      limit: input.limit,
      path: `/tracks/${trackUrn}/comments`,
      token,
    });
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
    input: PageInput,
  ): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: `track-favoriters:${trackUrn}`,
      ttl: TTL_FAVORITERS,
      page: input.page,
      limit: input.limit,
      path: `/tracks/${trackUrn}/favoriters`,
      token,
    });
  }

  getReposters(token: string, trackUrn: string, input: PageInput): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: `track-reposters:${trackUrn}`,
      ttl: TTL_REPOSTERS,
      page: input.page,
      limit: input.limit,
      path: `/tracks/${trackUrn}/reposters`,
      token,
    });
  }

  async getRelated(
    token: string,
    sessionId: string,
    trackUrn: string,
    input: PageInput,
    access: string,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: buildListCacheKey(`track-related:${trackUrn}`, { access }),
      ttl: TTL_RELATED,
      page: input.page,
      limit: input.limit,
      path: `/tracks/${trackUrn}/related`,
      token,
      extraParams: { access },
    });
    result.collection = await this.applyLocalLikeFlags(sessionId, result.collection);
    return result;
  }
}
