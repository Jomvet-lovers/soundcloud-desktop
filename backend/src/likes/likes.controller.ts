import { Controller, Delete, HttpCode, Param, Post, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { LikesService } from './likes.service.js';

@ApiTags('likes')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('likes')
export class LikesController {
  constructor(private readonly likesService: LikesService) {}

  @Post('tracks/:trackUrn')
  @HttpCode(200)
  @ApiOperation({ summary: 'Like a track' })
  likeTrack(@AccessToken() token: string, @Param('trackUrn') trackUrn: string) {
    return this.likesService.likeTrack(token, trackUrn);
  }

  @Delete('tracks/:trackUrn')
  @ApiOperation({ summary: 'Unlike a track' })
  unlikeTrack(@AccessToken() token: string, @Param('trackUrn') trackUrn: string) {
    return this.likesService.unlikeTrack(token, trackUrn);
  }

  @Post('playlists/:playlistUrn')
  @HttpCode(200)
  @ApiOperation({ summary: 'Like a playlist' })
  likePlaylist(@AccessToken() token: string, @Param('playlistUrn') playlistUrn: string) {
    return this.likesService.likePlaylist(token, playlistUrn);
  }

  @Delete('playlists/:playlistUrn')
  @ApiOperation({ summary: 'Unlike a playlist' })
  unlikePlaylist(@AccessToken() token: string, @Param('playlistUrn') playlistUrn: string) {
    return this.likesService.unlikePlaylist(token, playlistUrn);
  }
}
