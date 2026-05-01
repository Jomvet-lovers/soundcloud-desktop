import { Injectable } from '@nestjs/common';
import { buildListCacheKey, type ListPageResult } from '../cache/list-cache.service.js';
import { PendingActionsService } from '../pending-actions/pending-actions.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScPlaylist, ScTrack, ScUser } from '../soundcloud/soundcloud.types.js';
import { SoundcloudListService } from '../soundcloud/soundcloud-list.service.js';

const TTL_SEARCH = 300;
const TTL_TRACKS = 1800;
const TTL_REPOSTERS = 600;

interface PageInput {
  page: number;
  limit: number;
}

@Injectable()
export class PlaylistsService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly scList: SoundcloudListService,
    private readonly pendingActions: PendingActionsService,
  ) {}

  search(
    token: string,
    input: PageInput,
    extra: Record<string, unknown>,
  ): Promise<ListPageResult<ScPlaylist>> {
    return this.scList.getPage<ScPlaylist>({
      cacheKey: buildListCacheKey('playlists-search', extra),
      ttl: TTL_SEARCH,
      page: input.page,
      limit: input.limit,
      path: '/playlists',
      token,
      extraParams: extra,
    });
  }

  async create(token: string, sessionId: string, body: unknown): Promise<unknown> {
    try {
      return await this.sc.apiPost<ScPlaylist>('/playlists', token, body);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(
          sessionId,
          'playlist_create',
          'new',
          body as Record<string, unknown>,
        );
        return { queued: true, actionType: 'playlist_create' };
      }
      throw error;
    }
  }

  async getById(
    token: string,
    playlistUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPlaylist> {
    return this.sc.apiGet<ScPlaylist>(`/playlists/${playlistUrn}`, token, params);
  }

  async update(
    token: string,
    sessionId: string,
    playlistUrn: string,
    body: unknown,
  ): Promise<unknown> {
    try {
      return await this.sc.apiPut<ScPlaylist>(`/playlists/${playlistUrn}`, token, body);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(
          sessionId,
          'playlist_update',
          playlistUrn,
          body as Record<string, unknown>,
        );
        return { queued: true, actionType: 'playlist_update', targetUrn: playlistUrn };
      }
      throw error;
    }
  }

  async delete(token: string, sessionId: string, playlistUrn: string): Promise<unknown> {
    try {
      return await this.sc.apiDelete(`/playlists/${playlistUrn}`, token);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'playlist_delete', playlistUrn);
        return { queued: true, actionType: 'playlist_delete', targetUrn: playlistUrn };
      }
      throw error;
    }
  }

  getTracks(
    token: string,
    playlistUrn: string,
    input: PageInput,
    extra: Record<string, unknown>,
  ): Promise<ListPageResult<ScTrack>> {
    return this.scList.getPage<ScTrack>({
      cacheKey: buildListCacheKey(`playlist-tracks:${playlistUrn}`, extra),
      ttl: TTL_TRACKS,
      page: input.page,
      limit: input.limit,
      path: `/playlists/${playlistUrn}/tracks`,
      token,
      extraParams: extra,
    });
  }

  getReposters(
    token: string,
    playlistUrn: string,
    input: PageInput,
  ): Promise<ListPageResult<ScUser>> {
    return this.scList.getPage<ScUser>({
      cacheKey: `playlist-reposters:${playlistUrn}`,
      ttl: TTL_REPOSTERS,
      page: input.page,
      limit: input.limit,
      path: `/playlists/${playlistUrn}/reposters`,
      token,
    });
  }
}
