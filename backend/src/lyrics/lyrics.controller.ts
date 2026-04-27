import { Controller, Get, Param, Query } from '@nestjs/common';
import { ApiOperation, ApiQuery, ApiTags } from '@nestjs/swagger';
import { LyricsService } from './lyrics.service.js';

function parseDurationSec(duration?: string): number | undefined {
  if (!duration) return undefined;
  const parsed = Number(duration);
  if (!Number.isFinite(parsed)) return undefined;
  return parsed >= 10_000 ? Math.round(parsed / 1000) : Math.round(parsed);
}

@ApiTags('lyrics')
@Controller('lyrics')
export class LyricsController {
  constructor(private readonly lyrics: LyricsService) {}

  @Get('search')
  @ApiOperation({
    summary: 'Manual lyrics search by artist/title. Preview only — cache untouched.',
  })
  @ApiQuery({ name: 'artist', required: true })
  @ApiQuery({ name: 'title', required: true })
  @ApiQuery({ name: 'duration', required: false })
  async search(
    @Query('artist') artist: string,
    @Query('title') title: string,
    @Query('duration') duration?: string,
  ) {
    return this.lyrics.searchLyrics({
      artist: artist ?? '',
      title: title ?? '',
      durationSec: parseDurationSec(duration),
    });
  }

  @Get(':scTrackId')
  @ApiOperation({
    summary: 'Get lyrics for a track. Backend resolves artist/title from indexed data.',
  })
  async get(@Param('scTrackId') scTrackId: string) {
    return this.lyrics.ensureLyrics(scTrackId);
  }
}
