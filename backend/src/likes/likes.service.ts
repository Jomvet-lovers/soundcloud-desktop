import { Injectable } from '@nestjs/common';
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
}
