import {
  Body,
  Controller,
  Delete,
  Get,
  HttpCode,
  Param,
  Post,
  Query,
  UseGuards,
} from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { DislikesService } from './dislikes.service.js';

@ApiTags('dislikes')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('dislikes')
export class DislikesController {
  constructor(private readonly dislikes: DislikesService) {}

  @Post(':scTrackId')
  @HttpCode(200)
  @ApiOperation({ summary: 'Mark track as disliked' })
  add(
    @SessionId() sessionId: string,
    @Param('scTrackId') scTrackId: string,
    @Body() trackData?: Record<string, unknown>,
  ) {
    return this.dislikes.add(sessionId, scTrackId, trackData);
  }

  @Delete(':scTrackId')
  @ApiOperation({ summary: 'Remove dislike' })
  remove(@SessionId() sessionId: string, @Param('scTrackId') scTrackId: string) {
    return this.dislikes.remove(sessionId, scTrackId);
  }

  @Get('status/:scTrackId')
  @ApiOperation({ summary: 'Check if track is disliked' })
  async status(@SessionId() sessionId: string, @Param('scTrackId') scTrackId: string) {
    return { disliked: await this.dislikes.isDisliked(sessionId, scTrackId) };
  }

  @Get('ids')
  @ApiOperation({ summary: 'List disliked track IDs only (lightweight)' })
  async ids(@SessionId() sessionId: string): Promise<{ ids: string[] }> {
    return { ids: await this.dislikes.listIdsBySession(sessionId, 1000) };
  }

  @Get()
  @ApiOperation({ summary: 'List disliked tracks' })
  list(
    @SessionId() sessionId: string,
    @Query('limit') limit?: string,
    @Query('cursor') cursor?: string,
  ) {
    return this.dislikes.findAll(sessionId, Math.min(Number(limit) || 50, 200), cursor);
  }
}
