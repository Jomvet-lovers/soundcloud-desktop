import { Injectable } from '@nestjs/common';
import { ScPaginatedResponse, ScPlaylist } from '../soundcloud/soundcloud.types.js';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';

@Injectable()
export class LikesService {
  constructor(private readonly sc: SoundcloudService) {}

  likeTrack(token: string, trackUrn: string): Promise<unknown> {
    return this.sc.apiPost(`/likes/tracks/${trackUrn}`, token);
  }

  unlikeTrack(token: string, trackUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/likes/tracks/${trackUrn}`, token);
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
