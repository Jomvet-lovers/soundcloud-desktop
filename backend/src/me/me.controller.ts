import { Controller, Delete, Get, Param, Put, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { AuthService } from '../auth/auth.service.js';
import { CacheClear } from '../cache/cache-clear.decorator.js';
import { Cached } from '../cache/cached.decorator.js';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { PaginationQuery } from '../common/dto/pagination.dto.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import {
  PagedActivityResponse,
  PagedPlaylistResponse,
  PagedTrackResponse,
  PagedUserResponse,
  ScMe,
} from '../soundcloud/soundcloud.types.js';
import { SubscriptionsService } from '../subscriptions/subscriptions.service.js';
import { MeService } from './me.service.js';

@ApiTags('me')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('me')
export class MeController {
  constructor(
    private readonly meService: MeService,
    private readonly subscriptionsService: SubscriptionsService,
    private readonly authService: AuthService,
  ) {}

  @Get()
  @Cached({ ttl: 43200, scope: 'user' })
  @ApiOperation({ summary: 'Get authenticated user profile' })
  @ApiOkResponse({ type: ScMe })
  getProfile(@AccessToken() token: string) {
    return this.meService.getProfile(token);
  }

  @Get('subscription')
  @ApiOperation({ summary: 'Check if current user has an active subscription' })
  async getSubscription(@SessionId() sessionId: string) {
    const session = await this.authService.getSession(sessionId);
    const userUrn = session?.soundcloudUserId;
    const premium = userUrn ? await this.subscriptionsService.isPremium(userUrn) : false;
    return { premium };
  }

  @Get('feed')
  @ApiOperation({ summary: 'Get authenticated user feed' })
  @ApiOkResponse({ type: PagedActivityResponse })
  getFeed(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFeed(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get('feed/tracks')
  @ApiOperation({ summary: 'Get authenticated user track feed' })
  @ApiOkResponse({ type: PagedActivityResponse })
  getFeedTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFeedTracks(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get('likes/tracks')
  @ApiOperation({ summary: 'Get liked tracks' })
  @ApiQuery({
    name: 'access',
    required: false,
    enum: ['playable', 'preview', 'blocked'],
    default: ['playable', 'preview', 'blocked'],
  })
  @ApiOkResponse({ type: PagedTrackResponse })
  getLikedTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
    @Query('access') access: string = 'playable,preview,blocked',
  ) {
    return this.meService.getLikedTracks(
      token,
      sessionId,
      { page: query.page ?? 0, limit: query.limit ?? 30 },
      access,
    );
  }

  @Get('likes/playlists')
  @ApiOperation({ summary: 'Get liked playlists' })
  @ApiOkResponse({ type: PagedPlaylistResponse })
  getLikedPlaylists(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getLikedPlaylists(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get('followings')
  @ApiOperation({ summary: 'Get users followed by authenticated user' })
  @ApiOkResponse({ type: PagedUserResponse })
  getFollowings(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFollowings(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get('followings/tracks')
  @ApiOperation({ summary: 'Get tracks from followed users' })
  @ApiOkResponse({ type: PagedTrackResponse })
  getFollowingsTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFollowingsTracks(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Put('followings/:userUrn')
  @CacheClear('me-followings')
  @ApiOperation({ summary: 'Follow a user' })
  followUser(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.meService.followUser(token, userUrn);
  }

  @Delete('followings/:userUrn')
  @CacheClear('me-followings')
  @ApiOperation({ summary: 'Unfollow a user' })
  unfollowUser(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.meService.unfollowUser(token, userUrn);
  }

  @Get('followers')
  @ApiOperation({ summary: 'Get followers of authenticated user' })
  @ApiOkResponse({ type: PagedUserResponse })
  getFollowers(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getFollowers(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get('playlists')
  @ApiOperation({ summary: 'Get user playlists' })
  @ApiOkResponse({ type: PagedPlaylistResponse })
  getPlaylists(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getPlaylists(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get('tracks')
  @ApiOperation({ summary: 'Get user tracks' })
  @ApiOkResponse({ type: PagedTrackResponse })
  getTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
  ) {
    return this.meService.getTracks(token, sessionId, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }
}
