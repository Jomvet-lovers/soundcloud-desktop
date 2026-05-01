import { Controller, Get, Param, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { Cached } from '../cache/cached.decorator.js';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { PaginationQuery } from '../common/dto/pagination.dto.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import {
  PagedPlaylistResponse,
  PagedTrackResponse,
  PagedUserResponse,
  ScUser,
  ScWebProfile,
} from '../soundcloud/soundcloud.types.js';
import { SubscriptionsService } from '../subscriptions/subscriptions.service.js';
import { UsersService } from './users.service.js';

@ApiTags('users')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('users')
export class UsersController {
  constructor(
    private readonly usersService: UsersService,
    private readonly subscriptionsService: SubscriptionsService,
  ) {}

  @Get()
  @ApiOperation({ summary: 'Search users' })
  @ApiQuery({ name: 'q', required: false, description: 'Search query' })
  @ApiQuery({ name: 'ids', required: false, description: 'Comma-separated user IDs' })
  @ApiOkResponse({ type: PagedUserResponse })
  search(
    @AccessToken() token: string,
    @Query() query: PaginationQuery,
    @Query('q') q?: string,
    @Query('ids') ids?: string,
  ) {
    return this.usersService.search(
      token,
      { page: query.page ?? 0, limit: query.limit ?? 30 },
      q,
      ids,
    );
  }

  @Get(':userUrn')
  @Cached({ ttl: 3600 })
  @ApiOperation({ summary: 'Get user by URN' })
  @ApiOkResponse({ type: ScUser })
  getById(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.usersService.getById(token, userUrn);
  }

  @Get(':userUrn/followers')
  @ApiOperation({ summary: 'Get user followers' })
  @ApiOkResponse({ type: PagedUserResponse })
  getFollowers(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getFollowers(token, userUrn, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get(':userUrn/followings')
  @ApiOperation({ summary: 'Get user followings' })
  @ApiOkResponse({ type: PagedUserResponse })
  getFollowings(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getFollowings(token, userUrn, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get(':userUrn/followings/:followingUrn')
  @Cached({ ttl: 30 })
  @ApiOperation({ summary: 'Get user A is following to user B' })
  @ApiOkResponse({ type: Boolean, description: 'Returns true if following, otherwise false' })
  getIsFollowing(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Param('followingUrn') followingUrn: string,
  ) {
    return this.usersService.getIsFollowing(token, userUrn, followingUrn);
  }

  @Get(':userUrn/tracks')
  @ApiOperation({ summary: 'Get user tracks' })
  @ApiQuery({
    name: 'access',
    required: false,
    enum: ['playable', 'preview', 'blocked'],
    default: ['playable', 'preview', 'blocked'],
  })
  @ApiOkResponse({ type: PagedTrackResponse })
  getTracks(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
    @Query('access') access: string = 'playable,preview,blocked',
  ) {
    return this.usersService.getTracks(
      token,
      sessionId,
      userUrn,
      { page: query.page ?? 0, limit: query.limit ?? 30 },
      access,
    );
  }

  @Get(':userUrn/playlists')
  @ApiOperation({ summary: 'Get user playlists' })
  @ApiQuery({
    name: 'access',
    required: false,
    enum: ['playable', 'preview', 'blocked'],
    default: ['playable', 'preview', 'blocked'],
  })
  @ApiQuery({ name: 'show_tracks', required: false, type: Boolean })
  @ApiOkResponse({ type: PagedPlaylistResponse })
  getPlaylists(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
    @Query('access') access: string = 'playable,preview,blocked',
    @Query('show_tracks') showTracks?: string,
  ) {
    return this.usersService.getPlaylists(
      token,
      userUrn,
      { page: query.page ?? 0, limit: query.limit ?? 30 },
      access,
      showTracks,
    );
  }

  @Get(':userUrn/likes/tracks')
  @ApiOperation({ summary: 'Get user liked tracks' })
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
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
    @Query('access') access: string = 'playable,preview,blocked',
  ) {
    return this.usersService.getLikedTracks(
      token,
      sessionId,
      userUrn,
      { page: query.page ?? 0, limit: query.limit ?? 30 },
      access,
    );
  }

  @Get(':userUrn/likes/playlists')
  @ApiOperation({ summary: 'Get user liked playlists' })
  @ApiOkResponse({ type: PagedPlaylistResponse })
  getLikedPlaylists(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getLikedPlaylists(token, userUrn, {
      page: query.page ?? 0,
      limit: query.limit ?? 30,
    });
  }

  @Get(':userUrn/subscription')
  @Cached({ ttl: 300 })
  @ApiOperation({ summary: 'Check if a user has an active Star subscription' })
  @ApiOkResponse({ schema: { type: 'object', properties: { premium: { type: 'boolean' } } } })
  async getSubscription(@Param('userUrn') userUrn: string) {
    const premium = await this.subscriptionsService.isPremium(userUrn);
    return { premium };
  }

  @Get(':userUrn/web-profiles')
  @Cached({ ttl: 86400 })
  @ApiOperation({ summary: 'Get user web profiles' })
  @ApiOkResponse({ type: [ScWebProfile] })
  getWebProfiles(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.usersService.getWebProfiles(token, userUrn);
  }
}
