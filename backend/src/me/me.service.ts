import { Injectable } from '@nestjs/common';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import {
  ScActivity,
  ScMe,
  ScPaginatedResponse,
  ScPlaylist,
  ScTrack,
  ScUser,
} from '../soundcloud/soundcloud.types.js';

@Injectable()
export class MeService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly localLikes: LocalLikesService,
  ) {}

  getProfile(token: string): Promise<ScMe> {
    return this.sc.apiGet<ScMe>('/me', token);
  }

  getFeed(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScActivity>> {
    return this.sc.apiGet('/me/feed', token, params);
  }

  getFeedTracks(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScActivity>> {
    return this.sc.apiGet('/me/feed/tracks', token, params);
  }

  async getLikedTracks(
    token: string,
    sessionId: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    const scResult = await this.sc.apiGet<ScPaginatedResponse<ScTrack>>(
      '/me/likes/tracks',
      token,
      params,
    );

    // On the first page (no cursor/offset), prepend local likes
    const hasCursor = params && (params.cursor || params.offset);
    if (!hasCursor) {
      const localResult = await this.localLikes.findAll(sessionId, 200);
      if (localResult.collection.length > 0) {
        const scUrns = new Set(scResult.collection.map((t) => t.urn));
        const localTracks = localResult.collection
          .map((data) => data as unknown as ScTrack)
          .filter((t) => t.urn && !scUrns.has(t.urn));
        if (localTracks.length > 0) {
          scResult.collection = [...localTracks, ...scResult.collection];
        }
      }
    }

    return scResult;
  }

  getLikedPlaylists(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScPlaylist>> {
    return this.sc.apiGet('/me/likes/playlists', token, params);
  }

  getFollowings(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet('/me/followings', token, params);
  }

  getFollowingsTracks(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    return this.sc.apiGet('/me/followings/tracks', token, params);
  }

  followUser(token: string, userUrn: string): Promise<unknown> {
    return this.sc.apiPut(`/me/followings/${userUrn}`, token);
  }

  unfollowUser(token: string, userUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/me/followings/${userUrn}`, token);
  }

  getFollowers(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet('/me/followers', token, params);
  }

  getPlaylists(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScPlaylist>> {
    return this.sc.apiGet('/me/playlists', token, params);
  }

  getTracks(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    return this.sc.apiGet('/me/tracks', token, params);
  }
}
