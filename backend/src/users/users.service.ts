import { Injectable } from '@nestjs/common';
import { buildListCacheKey, type ListPageResult } from '../cache/list-cache.service.js';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScPlaylist, ScTrack, ScUser, ScWebProfile } from '../soundcloud/soundcloud.types.js';
import { SoundcloudListService } from '../soundcloud/soundcloud-list.service.js';

const TTL_SEARCH = 300;
const TTL_FOLLOWS = 600;
const TTL_USER_TRACKS = 600;
const TTL_USER_PLAYLISTS = 600;
const TTL_USER_LIKES = 600;

interface PageInput {
  page: number;
  limit: number;
}

@Injectable()
export class UsersService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly scList: SoundcloudListService,
    private readonly localLikes: LocalLikesService,
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

  search(
    token: string,
    input: PageInput,
    q?: string,
    ids?: string,
  ): Promise<ListPageResult<ScUser>> {
    const extra: Record<string, unknown> = {};
    if (q) extra.q = q;
    if (ids) extra.ids = ids;
    return this.scList.getPage<ScUser>({
      cacheKey: buildListCacheKey('users-search', extra),
      ttl: TTL_SEARCH,
      page: input.page,
      limit: input.limit,
      path: '/users',
      token,
      extraParams: extra,
    });
  }

  getById(token: string, userUrn: string): Promise<ScUser> {
    return this.sc.apiGet(`/users/${userUrn}`, token);
  }

  getFollowers(token: string, userUrn: string, input: PageInput): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: `user-followers:${userUrn}`,
      ttl: TTL_FOLLOWS,
      page: input.page,
      limit: input.limit,
      path: `/users/${userUrn}/followers`,
      token,
    });
  }

  getFollowings(token: string, userUrn: string, input: PageInput): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: `user-followings:${userUrn}`,
      ttl: TTL_FOLLOWS,
      page: input.page,
      limit: input.limit,
      path: `/users/${userUrn}/followings`,
      token,
    });
  }

  async getIsFollowing(token: string, userUrn: string, followingUrn: string): Promise<boolean> {
    try {
      const response = (await this.sc.apiGet(
        `/users/${userUrn}/followings/${followingUrn}`,
        token,
      )) as { urn?: string } | null;

      return response?.urn === followingUrn;
    } catch {
      return false;
    }
  }

  async getTracks(
    token: string,
    sessionId: string,
    userUrn: string,
    input: PageInput,
    access: string,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: buildListCacheKey(`user-tracks:${userUrn}`, { access }),
      ttl: TTL_USER_TRACKS,
      page: input.page,
      limit: input.limit,
      path: `/users/${userUrn}/tracks`,
      token,
      extraParams: { access },
    });
    result.collection = await this.applyLocalLikeFlags(sessionId, result.collection);
    return result;
  }

  getPlaylists(
    token: string,
    userUrn: string,
    input: PageInput,
    access: string,
    showTracks?: string,
  ): Promise<ListPageResult<ScPlaylist>> {
    const extra: Record<string, unknown> = { access };
    if (showTracks !== undefined) extra.show_tracks = showTracks;
    return this.scList.getPage<ScPlaylist>({
      cacheKey: buildListCacheKey(`user-playlists:${userUrn}`, extra),
      ttl: TTL_USER_PLAYLISTS,
      page: input.page,
      limit: input.limit,
      path: `/users/${userUrn}/playlists`,
      token,
      extraParams: extra,
    });
  }

  async getLikedTracks(
    token: string,
    sessionId: string,
    userUrn: string,
    input: PageInput,
    access: string,
  ): Promise<ListPageResult<ScTrack>> {
    const result = await this.scList.getPage<ScTrack>({
      cacheKey: buildListCacheKey(`user-liked-tracks:${userUrn}`, { access }),
      ttl: TTL_USER_LIKES,
      page: input.page,
      limit: input.limit,
      path: `/users/${userUrn}/likes/tracks`,
      token,
      extraParams: { access },
    });
    result.collection = await this.applyLocalLikeFlags(sessionId, result.collection);
    return result;
  }

  getLikedPlaylists(
    token: string,
    userUrn: string,
    input: PageInput,
  ): Promise<ListPageResult<ScPlaylist>> {
    return this.scList.getPage<ScPlaylist>({
      cacheKey: `user-liked-playlists:${userUrn}`,
      ttl: TTL_USER_LIKES,
      page: input.page,
      limit: input.limit,
      path: `/users/${userUrn}/likes/playlists`,
      token,
    });
  }

  getWebProfiles(
    token: string,
    userUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScWebProfile[]> {
    return this.sc.apiGet(`/users/${userUrn}/web-profiles`, token, params);
  }
}
