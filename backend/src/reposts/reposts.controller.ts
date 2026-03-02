import { Controller, Delete, HttpCode, Param, Post, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { RepostsService } from './reposts.service.js';

@ApiTags('reposts')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('reposts')
export class RepostsController {
  constructor(private readonly repostsService: RepostsService) {}

  @Post('tracks/:trackUrn')
  @HttpCode(201)
  @ApiOperation({ summary: 'Repost a track' })
  repostTrack(@AccessToken() token: string, @Param('trackUrn') trackUrn: string) {
    return this.repostsService.repostTrack(token, trackUrn);
  }

  @Delete('tracks/:trackUrn')
  @ApiOperation({ summary: 'Remove track repost' })
  removeTrackRepost(@AccessToken() token: string, @Param('trackUrn') trackUrn: string) {
    return this.repostsService.removeTrackRepost(token, trackUrn);
  }

  @Post('playlists/:playlistUrn')
  @HttpCode(201)
  @ApiOperation({ summary: 'Repost a playlist' })
  repostPlaylist(@AccessToken() token: string, @Param('playlistUrn') playlistUrn: string) {
    return this.repostsService.repostPlaylist(token, playlistUrn);
  }

  @Delete('playlists/:playlistUrn')
  @ApiOperation({ summary: 'Remove playlist repost' })
  removePlaylistRepost(@AccessToken() token: string, @Param('playlistUrn') playlistUrn: string) {
    return this.repostsService.removePlaylistRepost(token, playlistUrn);
  }
}
