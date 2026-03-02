import { Injectable } from '@nestjs/common';
import { SoundcloudService } from '../soundcloud/soundcloud.service.js';

@Injectable()
export class RepostsService {
  constructor(private readonly sc: SoundcloudService) {}

  repostTrack(token: string, trackUrn: string): Promise<unknown> {
    return this.sc.apiPost(`/reposts/tracks/${trackUrn}`, token);
  }

  removeTrackRepost(token: string, trackUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/reposts/tracks/${trackUrn}`, token);
  }

  repostPlaylist(token: string, playlistUrn: string): Promise<unknown> {
    return this.sc.apiPost(`/reposts/playlists/${playlistUrn}`, token);
  }

  removePlaylistRepost(token: string, playlistUrn: string): Promise<unknown> {
    return this.sc.apiDelete(`/reposts/playlists/${playlistUrn}`, token);
  }
}
