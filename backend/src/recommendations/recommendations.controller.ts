import { Controller, Get, Logger, Param, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { AuthService } from '../auth/auth.service.js';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { DislikesService } from '../dislikes/dislikes.service.js';
import { EventsService } from '../events/events.service.js';
import { RecommendationsService, type WaveMode } from './recommendations.service.js';

function parseMode(raw?: string): WaveMode {
  return raw === 'diverse' ? 'diverse' : 'similar';
}

function parseLimit(raw: string | undefined, fallback: number): number {
  const n = Number.parseInt(raw ?? '', 10);
  return Number.isFinite(n) && n > 0 ? n : fallback;
}

function parseLanguages(raw?: string): string[] | undefined {
  return raw ? raw.split(',').filter(Boolean) : undefined;
}

function parseDiversity(raw?: string): number {
  const v = Number.parseFloat(raw ?? '0');
  if (!Number.isFinite(v)) return 0;
  return Math.max(0, Math.min(1, v));
}

@ApiTags('recommendations')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('recommendations')
export class RecommendationsController {
  private readonly logger = new Logger(RecommendationsController.name);

  constructor(
    private readonly rec: RecommendationsService,
    private readonly events: EventsService,
    private readonly auth: AuthService,
    private readonly dislikes: DislikesService,
  ) {}

  private newReqId(): string {
    return Math.random().toString(36).slice(2, 8);
  }

  private async loadWaveContext(scUserId: string) {
    const [likedRecent, skipped, played, disliked] = await Promise.all([
      this.events.getRecentLiked(scUserId, 5),
      this.events.getRecentSkipped(scUserId, 3),
      this.events.getRecentPlayed(scUserId, 50),
      this.dislikes.listIdsByUserId(scUserId, 200),
    ]);
    const dislikedSet = new Set(disliked);
    const positive = likedRecent.filter((id) => !dislikedSet.has(id));
    const negative = [...new Set([...skipped, ...disliked])];
    const exclude = [...new Set([...played, ...disliked])];
    return { positive, negative, exclude };
  }

  @Get()
  @ApiOperation({ summary: 'SoundWave taste feed (no anchor). For Home initial load.' })
  @ApiQuery({ name: 'limit', required: false })
  @ApiQuery({ name: 'languages', required: false })
  @ApiQuery({ name: 'mode', required: false, enum: ['similar', 'diverse'] })
  async recommend(
    @SessionId() sessionId: string,
    @Query('limit') limitRaw?: string,
    @Query('languages') languagesRaw?: string,
    @Query('mode') modeRaw?: string,
  ) {
    const reqId = this.newReqId();
    const session = await this.auth.getSession(sessionId);
    const scUserId = session?.soundcloudUserId;
    if (!scUserId) {
      this.logger.log(`[${reqId}] GET /recommendations  no scUserId in session — returning []`);
      return [];
    }

    const limit = parseLimit(limitRaw, 20);
    const languages = parseLanguages(languagesRaw);
    const mode = parseMode(modeRaw);
    const { positive, negative, exclude } = await this.loadWaveContext(scUserId);

    this.logger.log(
      `[${reqId}] GET /recommendations  scUserId=${scUserId} mode=${mode} limit=${limit} ` +
        `langs=${languages?.join(',') ?? '*'} positive=${positive.length} ` +
        `negative=${negative.length} exclude=${exclude.length} modeRaw="${modeRaw ?? ''}"`,
    );

    const out = await this.rec.recommend(
      scUserId,
      positive,
      negative,
      exclude,
      limit,
      languages,
      mode,
      reqId,
    );
    this.logger.log(
      `[${reqId}] GET /recommendations  done  returned=${out.length} top5=[${out
        .slice(0, 5)
        .map((r) => r.id)
        .join(',')}]`,
    );
    return out;
  }

  @Get('wave/:seedTrackId')
  @ApiOperation({
    summary: 'SoundWave tail seeded by a specific track. Taste-aware, mode-aware.',
  })
  @ApiQuery({ name: 'limit', required: false })
  @ApiQuery({ name: 'languages', required: false })
  @ApiQuery({ name: 'mode', required: false, enum: ['similar', 'diverse'] })
  async wave(
    @SessionId() sessionId: string,
    @Param('seedTrackId') seedTrackId: string,
    @Query('limit') limitRaw?: string,
    @Query('languages') languagesRaw?: string,
    @Query('mode') modeRaw?: string,
  ) {
    const reqId = this.newReqId();
    const session = await this.auth.getSession(sessionId);
    const scUserId = session?.soundcloudUserId;
    if (!scUserId) {
      this.logger.log(`[${reqId}] GET /recommendations/wave  no scUserId — returning []`);
      return [];
    }

    const limit = parseLimit(limitRaw, 20);
    const languages = parseLanguages(languagesRaw);
    const mode = parseMode(modeRaw);
    const { positive, negative, exclude } = await this.loadWaveContext(scUserId);
    const fullExclude = [...new Set([...exclude, seedTrackId])];

    this.logger.log(
      `[${reqId}] GET /recommendations/wave/${seedTrackId}  scUserId=${scUserId} mode=${mode} ` +
        `limit=${limit} langs=${languages?.join(',') ?? '*'} positive=${positive.length} ` +
        `negative=${negative.length} exclude=${fullExclude.length} modeRaw="${modeRaw ?? ''}"`,
    );

    const out = await this.rec.wave(
      scUserId,
      seedTrackId,
      positive,
      negative,
      fullExclude,
      limit,
      languages,
      mode,
      reqId,
    );
    this.logger.log(
      `[${reqId}] GET /recommendations/wave  done  returned=${out.length} top5=[${out
        .slice(0, 5)
        .map((r) => r.id)
        .join(',')}]`,
    );
    return out;
  }

  @Get('similar/:trackId')
  @ApiOperation({
    summary: 'Pure similar-by-track. No user taste, no user history. For TrackPage.',
  })
  @ApiQuery({ name: 'limit', required: false })
  @ApiQuery({ name: 'languages', required: false })
  @ApiQuery({ name: 'diversity', required: false })
  @ApiQuery({ name: 'exclude', required: false })
  async similar(
    @Param('trackId') trackId: string,
    @Query('exclude') excludeRaw?: string,
    @Query('limit') limitRaw?: string,
    @Query('languages') languagesRaw?: string,
    @Query('diversity') diversityRaw?: string,
  ) {
    const reqId = this.newReqId();
    const clientExclude = excludeRaw ? excludeRaw.split(',') : [];
    const exclude = [...new Set([...clientExclude, trackId])];
    const limit = parseLimit(limitRaw, 10);
    const languages = parseLanguages(languagesRaw);
    const diversity = parseDiversity(diversityRaw);
    this.logger.log(
      `[${reqId}] GET /recommendations/similar/${trackId}  diversity=${diversity} limit=${limit} ` +
        `langs=${languages?.join(',') ?? '*'} exclude=${exclude.length}`,
    );
    const out = await this.rec.similar(trackId, exclude, limit, languages, diversity);
    this.logger.log(
      `[${reqId}] GET /recommendations/similar  done  returned=${out.length} top5=[${out
        .slice(0, 5)
        .map((r) => r.id)
        .join(',')}]`,
    );
    return out;
  }

  @Get('search')
  @ApiOperation({ summary: 'Search audio by text description (MuQ-MuLan)' })
  @ApiQuery({ name: 'q', required: true })
  @ApiQuery({ name: 'limit', required: false })
  @ApiQuery({ name: 'languages', required: false })
  async searchByText(
    @Query('q') q: string,
    @Query('limit') limitRaw?: string,
    @Query('languages') languagesRaw?: string,
  ) {
    const limit = parseLimit(limitRaw, 20);
    const languages = parseLanguages(languagesRaw);
    return this.rec.searchByText(q ?? '', limit, languages);
  }
}
