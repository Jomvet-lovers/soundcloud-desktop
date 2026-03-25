import { Injectable } from '@nestjs/common';
import { PendingActionsService } from '../pending-actions/pending-actions.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import {
  ScPaginatedResponse,
  ScPlaylist,
  ScTrack,
  ScUser,
} from '../soundcloud/soundcloud.types.js';

@Injectable()
export class PlaylistsService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly pendingActions: PendingActionsService,
  ) {}

  search(
    token: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScPlaylist>> {
    return this.sc.apiGet('/playlists', token, params);
  }

  async create(token: string, sessionId: string, body: unknown): Promise<unknown> {
    try {
      return await this.sc.apiPost<ScPlaylist>('/playlists', token, body);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'playlist_create', 'new', body as Record<string, unknown>);
        return { queued: true, actionType: 'playlist_create' };
      }
      throw error;
    }
  }

  getById(
    token: string,
    playlistUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPlaylist> {
    return this.sc.apiGet(`/playlists/${playlistUrn}`, token, params);
  }

  async update(token: string, sessionId: string, playlistUrn: string, body: unknown): Promise<unknown> {
    try {
      return await this.sc.apiPut<ScPlaylist>(`/playlists/${playlistUrn}`, token, body);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'playlist_update', playlistUrn, body as Record<string, unknown>);
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
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScTrack>> {
    return this.sc.apiGet(`/playlists/${playlistUrn}/tracks`, token, params);
  }

  getReposters(
    token: string,
    playlistUrn: string,
    params?: Record<string, unknown>,
  ): Promise<ScPaginatedResponse<ScUser>> {
    return this.sc.apiGet(`/playlists/${playlistUrn}/reposters`, token, params);
  }
}
