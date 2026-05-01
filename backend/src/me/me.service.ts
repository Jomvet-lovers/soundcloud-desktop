import { Injectable, Logger } from '@nestjs/common';
import { AuthService } from '../auth/auth.service.js';
import { buildListCacheKey, type ListPageResult } from '../cache/list-cache.service.js';
import { EventsService } from '../events/events.service.js';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScActivity, ScMe, ScPlaylist, ScTrack, ScUser } from '../soundcloud/soundcloud.types.js';
import { SoundcloudListService } from '../soundcloud/soundcloud-list.service.js';

const TTL_FEED = 60;
const TTL_LIKES_TRACKS = 1800;
const TTL_LIKES_PLAYLISTS = 1800;
const TTL_FOLLOWINGS = 3600;
const TTL_FOLLOWINGS_TRACKS = 60;
const TTL_FOLLOWERS = 600;
const TTL_PLAYLISTS = 3600;
const TTL_TRACKS = 120;

interface PageInput {
  page: number;
  limit: number;
}

@Injectable()
export class MeService {
  private readonly logger = new Logger(MeService.name);

  constructor(
    private readonly sc: SoundcloudService,
    private readonly scList: SoundcloudListService,
    private readonly localLikes: LocalLikesService,
    private readonly auth: AuthService,
    private readonly events: EventsService,
  ) {}

  getProfile(token: string): Promise<ScMe> {
    return this.sc.apiGet<ScMe>('/me', token);
  }

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

  private async applyLocalLikeFlagsToActivities(
    sessionId: string,
    activities: ScActivity[],
  ): Promise<ScActivity[]> {
    const trackOrigins = activities
      .map((activity) => activity.origin)
      .filter((origin): origin is ScTrack => origin?.kind === 'track');

    const annotatedTracks = await this.applyLocalLikeFlags(sessionId, trackOrigins);
    const byUrn = new Map(annotatedTracks.map((track) => [track.urn, track]));

    return activities.map((activity) => {
      if (activity.origin?.kind !== 'track') {
        return activity;
      }
      return {
        ...activity,
        origin: byUrn.get(activity.origin.urn) ?? activity.origin,
      };
    });
  }

  async getFeed(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScActivity>> {
    const result = await this.scList.getPage<ScActivity>({
      cacheKey: 'me-feed',
      ttl: TTL_FEED,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/feed',
      token,
    });
    result.collection = await this.applyLocalLikeFlagsToActivities(sessionId, result.collection);
    return result;
  }

  async getFeedTracks(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScActivity>> {
    const result = await this.scList.getPage<ScActivity>({
      cacheKey: 'me-feed-tracks',
      ttl: TTL_FEED,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/feed/tracks',
      token,
    });
    result.collection = await this.applyLocalLikeFlagsToActivities(sessionId, result.collection);
    return result;
  }

  async getLikedTracks(
    token: string,
    sessionId: string,
    input: PageInput,
    access: string,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: buildListCacheKey('me-liked-tracks', { access }),
      ttl: TTL_LIKES_TRACKS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/likes/tracks',
      token,
      extraParams: { access },
    });

    if (input.page === 0) {
      const localResult = await this.localLikes.findAll(sessionId, 200);
      if (localResult.collection.length > 0) {
        const scUrns = new Set(result.collection.map((t) => t.urn));
        const localTracks = localResult.collection
          .map((data) => data as unknown as ScTrack)
          .filter((t) => t.urn && !scUrns.has(t.urn));
        if (localTracks.length > 0) {
          result.collection = [...localTracks, ...result.collection];
        }
      }
    }

    this.seedLikesTaste(sessionId, result.collection).catch((e) => {
      this.logger.debug(`seedLikesTaste failed: ${(e as Error).message}`);
    });

    return result;
  }

  private async seedLikesTaste(sessionId: string, tracks: ScTrack[]): Promise<void> {
    if (tracks.length === 0) return;
    const session = await this.auth.getSession(sessionId);
    const scUserId = session?.soundcloudUserId;
    if (!scUserId) return;
    const trackIds = tracks.map((t) => t.urn).filter(Boolean);
    await this.events.ensureLikesRecorded(scUserId, trackIds);
  }

  getLikedPlaylists(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScPlaylist>> {
    return this.scList.getPage<ScPlaylist>({
      cacheKey: 'me-liked-playlists',
      ttl: TTL_LIKES_PLAYLISTS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/likes/playlists',
      token,
    });
  }

  getFollowings(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: 'me-followings',
      ttl: TTL_FOLLOWINGS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/followings',
      token,
    });
  }

  async getFollowingsTracks(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: 'me-followings-tracks',
      ttl: TTL_FOLLOWINGS_TRACKS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/followings/tracks',
      token,
    });
    result.collection = await this.applyLocalLikeFlags(sessionId, result.collection);
    return result;
  }

  followUser(token: string, userUrn: string): Promise<unknown> {
    return this.sc.apiPut(`/me/followings/${userUrn}`, token);
  }

  unfollowUser(token: string, userUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/me/followings/${userUrn}`, token);
  }

  getFollowers(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: 'me-followers',
      ttl: TTL_FOLLOWERS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/followers',
      token,
    });
  }

  getPlaylists(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScPlaylist>> {
    return this.scList.getPage<ScPlaylist>({
      cacheKey: 'me-playlists',
      ttl: TTL_PLAYLISTS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/playlists',
      token,
    });
  }

  async getTracks(
    token: string,
    sessionId: string,
    input: PageInput,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: 'me-tracks',
      ttl: TTL_TRACKS,
      scope: 'user',
      sessionId,
      page: input.page,
      limit: input.limit,
      path: '/me/tracks',
      token,
    });
    result.collection = await this.applyLocalLikeFlags(sessionId, result.collection);
    return result;
  }
}
