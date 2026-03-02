import { Controller, Get, Param, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { PaginationQuery } from '../common/dto/pagination.dto.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { UsersService } from './users.service.js';

@ApiTags('users')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('users')
export class UsersController {
  constructor(private readonly usersService: UsersService) {}

  @Get()
  @ApiOperation({ summary: 'Search users' })
  @ApiQuery({ name: 'q', required: false, description: 'Search query' })
  @ApiQuery({ name: 'ids', required: false, description: 'Comma-separated user IDs' })
  search(
    @AccessToken() token: string,
    @Query() query: PaginationQuery,
    @Query('q') q?: string,
    @Query('ids') ids?: string,
  ) {
    const params: Record<string, unknown> = { ...query };
    if (q) params.q = q;
    if (ids) params.ids = ids;
    return this.usersService.search(token, params);
  }

  @Get(':userUrn')
  @ApiOperation({ summary: 'Get user by URN' })
  getById(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.usersService.getById(token, userUrn);
  }

  @Get(':userUrn/followers')
  @ApiOperation({ summary: 'Get user followers' })
  getFollowers(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getFollowers(token, userUrn, query as Record<string, unknown>);
  }

  @Get(':userUrn/followings')
  @ApiOperation({ summary: 'Get user followings' })
  getFollowings(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getFollowings(token, userUrn, query as Record<string, unknown>);
  }

  @Get(':userUrn/tracks')
  @ApiOperation({ summary: 'Get user tracks' })
  @ApiQuery({ name: 'access', required: false, enum: ['playable', 'preview', 'blocked'] })
  getTracks(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getTracks(token, userUrn, query as Record<string, unknown>);
  }

  @Get(':userUrn/playlists')
  @ApiOperation({ summary: 'Get user playlists' })
  @ApiQuery({ name: 'access', required: false, enum: ['playable', 'preview', 'blocked'] })
  @ApiQuery({ name: 'show_tracks', required: false, type: Boolean })
  getPlaylists(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getPlaylists(token, userUrn, query as Record<string, unknown>);
  }

  @Get(':userUrn/likes/tracks')
  @ApiOperation({ summary: 'Get user liked tracks' })
  @ApiQuery({ name: 'access', required: false, enum: ['playable', 'preview', 'blocked'] })
  getLikedTracks(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getLikedTracks(token, userUrn, query as Record<string, unknown>);
  }

  @Get(':userUrn/likes/playlists')
  @ApiOperation({ summary: 'Get user liked playlists' })
  getLikedPlaylists(
    @AccessToken() token: string,
    @Param('userUrn') userUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.usersService.getLikedPlaylists(token, userUrn, query as Record<string, unknown>);
  }

  @Get(':userUrn/web-profiles')
  @ApiOperation({ summary: 'Get user web profiles' })
  getWebProfiles(@AccessToken() token: string, @Param('userUrn') userUrn: string) {
    return this.usersService.getWebProfiles(token, userUrn);
  }
}
