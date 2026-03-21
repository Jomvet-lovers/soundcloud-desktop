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
import { LocalLikesService } from './local-likes.service.js';

@ApiTags('local-likes')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('local-likes')
export class LocalLikesController {
  constructor(private readonly localLikesService: LocalLikesService) {}

  @Post(':scTrackId')
  @HttpCode(200)
  @ApiOperation({ summary: 'Add a local like' })
  add(
    @SessionId() sessionId: string,
    @Param('scTrackId') scTrackId: string,
    @Body() trackData: Record<string, unknown>,
  ) {
    return this.localLikesService.add(sessionId, scTrackId, trackData);
  }

  @Delete(':scTrackId')
  @ApiOperation({ summary: 'Remove a local like' })
  remove(@SessionId() sessionId: string, @Param('scTrackId') scTrackId: string) {
    return this.localLikesService.remove(sessionId, scTrackId);
  }

  @Get()
  @ApiOperation({ summary: 'Get all local likes' })
  findAll(
    @SessionId() sessionId: string,
    @Query('limit') limit?: string,
    @Query('cursor') cursor?: string,
  ) {
    return this.localLikesService.findAll(sessionId, Math.min(Number(limit) || 50, 200), cursor);
  }
}
