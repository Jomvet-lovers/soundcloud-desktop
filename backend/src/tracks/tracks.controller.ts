import {
  Body,
  Controller,
  Delete,
  Get,
  Param,
  Post,
  Put,
  Query,
  Res,
  UseGuards,
} from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import {
  ApiBody,
  ApiHeader,
  ApiOkResponse,
  ApiOperation,
  ApiQuery,
  ApiTags,
} from '@nestjs/swagger';
import { CacheClear } from '../cache/cache-clear.decorator.js';
import { Cached } from '../cache/cached.decorator.js';
import { AccessToken } from '../common/decorators/access-token.decorator.js';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { PaginationQuery } from '../common/dto/pagination.dto.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import {
  PaginatedCommentResponse,
  PaginatedTrackResponse,
  PaginatedUserResponse,
  ScComment,
  ScStreams,
  ScTrack,
} from '../soundcloud/soundcloud.types.js';
import { TracksService } from './tracks.service.js';

@ApiTags('tracks')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('tracks')
export class TracksController {
  private readonly streamingServiceUrl: string;

  constructor(
    private readonly tracksService: TracksService,
    configService: ConfigService,
  ) {
    this.streamingServiceUrl =
      configService.get<string>('streaming.serviceUrl') ?? 'http://localhost:8080';
  }

  @Get()
  @Cached({ ttl: 60 })
  @ApiOperation({ summary: 'Search tracks' })
  @ApiQuery({ name: 'q', required: false, description: 'Search query' })
  @ApiQuery({ name: 'ids', required: false, description: 'Comma-separated track IDs' })
  @ApiQuery({ name: 'genres', required: false, description: 'Comma-separated genres' })
  @ApiQuery({ name: 'tags', required: false, description: 'Comma-separated tags' })
  @ApiQuery({
    name: 'access',
    required: false,
    enum: ['playable', 'preview', 'blocked'],
    default: ['playable', 'preview', 'blocked'],
  })
  @ApiOkResponse({ type: PaginatedTrackResponse })
  search(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Query() query: PaginationQuery,
    @Query('q') q?: string,
    @Query('ids') ids?: string,
    @Query('genres') genres?: string,
    @Query('tags') tags?: string,
    @Query('access') access: string = 'playable,preview,blocked',
  ) {
    const params: Record<string, unknown> = { ...query, access };
    if (q) params.q = q;
    if (ids) params.ids = ids;
    if (genres) params.genres = genres;
    if (tags) params.tags = tags;
    return this.tracksService.search(token, sessionId, params);
  }

  @Get(':trackUrn')
  @Cached({ ttl: 600 })
  @ApiOperation({ summary: 'Get track by URN' })
  @ApiQuery({ name: 'secret_token', required: false })
  @ApiOkResponse({ type: ScTrack })
  getById(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Param('trackUrn') trackUrn: string,
    @Query('secret_token') secretToken?: string,
  ) {
    const params: Record<string, unknown> = {};
    if (secretToken) params.secret_token = secretToken;
    return this.tracksService.getById(token, sessionId, trackUrn, params);
  }

  @Put(':trackUrn')
  @ApiOperation({ summary: 'Update track metadata' })
  @ApiOkResponse({ type: ScTrack })
  update(
    @AccessToken() token: string,
    @Param('trackUrn') trackUrn: string,
    @Body() body: Record<string, unknown>,
  ) {
    return this.tracksService.update(token, trackUrn, body);
  }

  @Delete(':trackUrn')
  @ApiOperation({ summary: 'Delete a track' })
  delete(@AccessToken() token: string, @Param('trackUrn') trackUrn: string) {
    return this.tracksService.delete(token, trackUrn);
  }

  @Get(':trackUrn/streams')
  @Cached({ ttl: 3600 })
  @ApiOperation({ summary: 'Get track stream URLs' })
  @ApiQuery({
    name: 'secret_token',
    required: false,
    description: 'Token for accessing private tracks (the s-xxx part from private share URLs)',
  })
  @ApiOkResponse({ type: ScStreams })
  getStreams(
    @AccessToken() token: string,
    @Param('trackUrn') trackUrn: string,
    @Query('secret_token') secretToken?: string,
  ) {
    const params: Record<string, unknown> = {};
    if (secretToken) params.secret_token = secretToken;
    return this.tracksService.getStreams(token, trackUrn, params);
  }

  @Get(':trackUrn/stream')
  @ApiOperation({
    summary: 'Redirect to streaming service',
    description: 'Redirects to the dedicated streaming service.',
  })
  @ApiQuery({ name: 'secret_token', required: false })
  @ApiQuery({ name: 'hq', required: false })
  proxyStream(
    @Res() res: any,
    @SessionId() sessionId: string,
    @Param('trackUrn') trackUrn: string,
    @Query('secret_token') secretToken?: string,
    @Query('hq') hq?: string,
  ) {
    const params = new URLSearchParams();
    params.set('session_id', sessionId);
    if (secretToken) params.set('secret_token', secretToken);
    if (hq) params.set('hq', hq);
    const url = `${this.streamingServiceUrl}/stream/${encodeURIComponent(trackUrn)}?${params.toString()}`;
    res.redirect(301, url);
  }

  @Get(':trackUrn/comments')
  @Cached({ ttl: 120, key: 'track-comments' })
  @ApiOperation({ summary: 'Get track comments' })
  @ApiOkResponse({ type: PaginatedCommentResponse })
  getComments(
    @AccessToken() token: string,
    @Param('trackUrn') trackUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.tracksService.getComments(token, trackUrn, query as Record<string, unknown>);
  }

  @Post(':trackUrn/comments')
  @CacheClear('track-comments')
  @ApiOperation({ summary: 'Post a comment on a track' })
  @ApiBody({
    schema: {
      type: 'object',
      properties: {
        comment: {
          type: 'object',
          properties: {
            body: { type: 'string' },
            timestamp: { type: 'number' },
          },
          required: ['body'],
        },
      },
    },
  })
  @ApiOkResponse({ type: ScComment })
  createComment(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Param('trackUrn') trackUrn: string,
    @Body() body: { comment: { body: string; timestamp?: number } },
  ) {
    return this.tracksService.createComment(token, sessionId, trackUrn, body);
  }

  @Get(':trackUrn/favoriters')
  @Cached({ ttl: 600 })
  @ApiOperation({ summary: 'Get users who favorited a track' })
  @ApiOkResponse({ type: PaginatedUserResponse })
  getFavoriters(
    @AccessToken() token: string,
    @Param('trackUrn') trackUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.tracksService.getFavoriters(token, trackUrn, query as Record<string, unknown>);
  }

  @Get(':trackUrn/reposters')
  @Cached({ ttl: 600 })
  @ApiOperation({ summary: 'Get users who reposted a track' })
  @ApiOkResponse({ type: PaginatedUserResponse })
  getReposters(
    @AccessToken() token: string,
    @Param('trackUrn') trackUrn: string,
    @Query() query: PaginationQuery,
  ) {
    return this.tracksService.getReposters(token, trackUrn, query as Record<string, unknown>);
  }

  @Get(':trackUrn/related')
  @Cached({ ttl: 86400 })
  @ApiOperation({ summary: 'Get related tracks' })
  @ApiQuery({
    name: 'access',
    required: false,
    enum: ['playable', 'preview', 'blocked'],
    default: ['playable', 'preview', 'blocked'],
  })
  @ApiOkResponse({ type: PaginatedTrackResponse })
  getRelated(
    @AccessToken() token: string,
    @SessionId() sessionId: string,
    @Param('trackUrn') trackUrn: string,
    @Query() query: PaginationQuery,
    @Query('access') access: string = 'playable,preview,blocked',
  ) {
    const params: Record<string, unknown> = { ...query, access };
    return this.tracksService.getRelated(token, sessionId, trackUrn, params);
  }
}
