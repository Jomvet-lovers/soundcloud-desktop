import { Controller, Delete, Get, Param, Put, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { Cached } from '../cache/cached.decorator.js';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { PaginationQuery } from '../common/dto/pagination.dto.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import {
  PaginatedActivityResponse,
  PaginatedPlaylistResponse,
  PaginatedTrackResponse,
  PaginatedUserResponse,
  ScMe,
} from '../soundcloud/soundcloud.types.js';
import { MeService } from './me.service.js';

@ApiTags('me')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('me')
export class MeController {
  constructor(private readonly meService: MeService) {}

  @Get()
  @Cached({ ttl: 30, scope: 'user' })
  @ApiOperation({ summary: 'Get authenticated user profile' })
  @ApiOkResponse({ type: ScMe })
  getProfile(@AccessToken() token: string) {
    return this.meService.getProfile(token);
  }

  @Get('feed')
  @Cached({ ttl: 60, scope: 'user' })
  @ApiOperation({ summary: 'Get authenticated user feed' })
  @ApiOkResponse({ type: PaginatedActivityResponse })
  getFeed(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFeed(token, sessionId, query as Record<string, unknown>);
  }

  @Get('feed/tracks')
  @Cached({ ttl: 60, scope: 'user' })
  @ApiOperation({ summary: 'Get authenticated user track feed' })
  @ApiOkResponse({ type: PaginatedActivityResponse })
  getFeedTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFeedTracks(token, sessionId, query as Record<string, unknown>);
  }

  @Get('likes/tracks')
  @Cached({ ttl: 30, scope: 'user', key: 'me-liked-tracks' })
  @ApiOperation({ summary: 'Get liked tracks' })
  @ApiQuery({
    name: 'access',
    required: false,
    enum: ['playable', 'preview', 'blocked'],
    default: ['playable', 'preview', 'blocked'],
  })
  @ApiOkResponse({ type: PaginatedTrackResponse })
  getLikedTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
    @Query('access') access: string = 'playable,preview,blocked',
  ) {
    const params: Record<string, unknown> = { ...query, access };
    return this.meService.getLikedTracks(token, sessionId, params);
  }

  @Get('likes/playlists')
  @Cached({ ttl: 30, scope: 'user', key: 'me-liked-playlists' })
  @ApiOperation({ summary: 'Get liked playlists' })
  @ApiOkResponse({ type: PaginatedPlaylistResponse })
  getLikedPlaylists(@AccessToken() token: string, @Query() query: PaginationQuery) {
    return this.meService.getLikedPlaylists(token, query as Record<string, unknown>);
  }

  @Get('followings')
  @Cached({ ttl: 5, scope: 'user' })
  @ApiOperation({ summary: 'Get users followed by authenticated user' })
  @ApiOkResponse({ type: PaginatedUserResponse })
  getFollowings(@AccessToken() token: string, @Query() query: PaginationQuery) {
    return this.meService.getFollowings(token, query as Record<string, unknown>);
  }

  @Get('followings/tracks')
  @Cached({ ttl: 30, scope: 'user' })
  @ApiOperation({ summary: 'Get tracks from followed users' })
  @ApiOkResponse({ type: PaginatedTrackResponse })
  getFollowingsTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFollowingsTracks(token, sessionId, query as Record<string, unknown>);
  }

  @Put('followings/:userUrn')
  @ApiOperation({ summary: 'Follow a user' })
  followUser(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.meService.followUser(token, userUrn);
  }

  @Delete('followings/:userUrn')
  @ApiOperation({ summary: 'Unfollow a user' })
  unfollowUser(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.meService.unfollowUser(token, userUrn);
  }

  @Get('followers')
  @Cached({ ttl: 120, scope: 'user' })
  @ApiOperation({ summary: 'Get followers of authenticated user' })
  @ApiOkResponse({ type: PaginatedUserResponse })
  getFollowers(@AccessToken() token: string, @Query() query: PaginationQuery) {
    return this.meService.getFollowers(token, query as Record<string, unknown>);
  }

  @Get('playlists')
  @ApiOperation({ summary: 'Get user playlists' })
  @ApiQuery({ name: 'show_tracks', required: false, type: Boolean })
  @ApiOkResponse({ type: PaginatedPlaylistResponse })
  getPlaylists(@AccessToken() token: string, @Query() query: PaginationQuery) {
    return this.meService.getPlaylists(token, query as Record<string, unknown>);
  }

  @Get('tracks')
  @Cached({ ttl: 30, scope: 'user' })
  @ApiOperation({ summary: 'Get user tracks' })
  @ApiOkResponse({ type: PaginatedTrackResponse })
  getTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getTracks(token, sessionId, query as Record<string, unknown>);
  }
}
