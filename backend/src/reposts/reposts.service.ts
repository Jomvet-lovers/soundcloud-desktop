import { Injectable } from '@nestjs/common';
import { PendingActionsService } from '../pending-actions/pending-actions.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';

@Injectable()
export class RepostsService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly pendingActions: PendingActionsService,
  ) {}

  async repostTrack(token: string, sessionId: string, trackUrn: string): Promise<unknown> {
    try {
      return await this.sc.apiPost(`/reposts/tracks/${trackUrn}`, token);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'repost', trackUrn);
        return { queued: true, actionType: 'repost', targetUrn: trackUrn };
      }
      throw error;
    }
  }

  async removeTrackRepost(token: string, sessionId: string, trackUrn: string): Promise<unknown> {
    try {
      return await this.sc.apiDelete(`/reposts/tracks/${trackUrn}`, token);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'unrepost', trackUrn);
        return { queued: true, actionType: 'unrepost', targetUrn: trackUrn };
      }
      throw error;
    }
  }

  async repostPlaylist(token: string, sessionId: string, playlistUrn: string): Promise<unknown> {
    try {
      return await this.sc.apiPost(`/reposts/playlists/${playlistUrn}`, token);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'repost_playlist', playlistUrn);
        return { queued: true, actionType: 'repost_playlist', targetUrn: playlistUrn };
      }
      throw error;
    }
  }

  async removePlaylistRepost(token: string, sessionId: string, playlistUrn: string): Promise<unknown> {
    try {
      return await this.sc.apiDelete(`/reposts/playlists/${playlistUrn}`, token);
    } catch (error) {
      if (this.pendingActions.isBanError(error)) {
        await this.pendingActions.enqueue(sessionId, 'unrepost_playlist', playlistUrn);
        return { queued: true, actionType: 'unrepost_playlist', targetUrn: playlistUrn };
      }
      throw error;
    }
  }
}
