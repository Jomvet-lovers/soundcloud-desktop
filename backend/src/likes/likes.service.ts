import { Injectable } from '@nestjs/common';
import { LocalLikesService } from '../local-likes/local-likes.service.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';
import { ScPaginatedResponse, ScPlaylist } from '../soundcloud/soundcloud.types.js';

@Injectable()
export class LikesService {
  constructor(
    private readonly sc: SoundcloudService,
    private readonly localLikes: LocalLikesService,
  ) {}

  async likeTrack(
    token: string,
    sessionId: string,
    trackUrn: string,
    trackData?: Record<string, unknown>,
  ): Promise<unknown> {
    try {
      return await this.sc.apiPost(`/likes/tracks/${trackUrn}`, token);
    } catch {
      // SC API failed (track removed, rate limit, etc.) — save as local like
      if (trackData) {
        await this.localLikes.add(sessionId, trackUrn, trackData);
      }
      return { status: 'local' };
    }
  }

  async unlikeTrack(token: string, sessionId: string, trackUrn: string): Promise<unknown> {
    // Delete from both SC and local
    const results = await Promise.allSettled([
      this.sc.apiDelete(`/likes/tracks/${trackUrn}`, token),
      this.localLikes.remove(sessionId, trackUrn),
    ]);
    // If SC succeeded, return its result
    if (results[0].status === 'fulfilled') return results[0].value;
    return { status: 'removed' };
  }

  async isLocalLiked(sessionId: string, trackUrn: string): Promise<boolean> {
    return this.localLikes.isLiked(sessionId, trackUrn);
  }

  likePlaylist(token: string, playlistUrn: string): Promise<unknown> {
    return this.sc.apiPost(`/likes/playlists/${playlistUrn}`, token);
  }

  unlikePlaylist(token: string, playlistUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/likes/playlists/${playlistUrn}`, token);
  }

  async isPlaylistLiked(token: string, playlistUrn: string): Promise<{ liked: boolean }> {
    let cursor: string | undefined;
    for (;;) {
      const params: Record<string, unknown> = { limit: 200, linked_partitioning: true };
      if (cursor) params.cursor = cursor;
      const page = await this.sc.apiGet<ScPaginatedResponse<ScPlaylist>>(
        '/me/likes/playlists',
        token,
        params,
      );
      if (!page?.collection) break;
      if (page.collection.some((p) => p.urn === playlistUrn)) {
        return { liked: true };
      }
      if (!page.next_href) break;
      const url = new URL(page.next_href);
      cursor = url.searchParams.get('cursor') ?? undefined;
      if (!cursor) break;
    }
    return { liked: false };
  }
}
