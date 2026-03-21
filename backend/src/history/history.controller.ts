import { Body, Controller, Delete, Get, HttpCode, Post, Query, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { IsInt, IsNotEmpty, IsOptional, IsString } from 'class-validator';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { HistoryService } from './history.service.js';

class RecordHistoryDto {
  @IsString()
  @IsNotEmpty()
  scTrackId: string;

  @IsString()
  @IsNotEmpty()
  title: string;

  @IsString()
  artistName: string;

  @IsString()
  @IsOptional()
  artworkUrl?: string;

  @IsInt()
  duration: number;
}

@ApiTags('history')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('history')
export class HistoryController {
  constructor(private readonly historyService: HistoryService) {}

  @Post()
  @HttpCode(200)
  @ApiOperation({ summary: 'Record a track play' })
  record(@SessionId() sessionId: string, @Body() body: RecordHistoryDto) {
    return this.historyService.record(sessionId, body);
  }

  @Get()
  @ApiOperation({ summary: 'Get listening history' })
  findAll(
    @SessionId() sessionId: string,
    @Query('limit') limit?: string,
    @Query('offset') offset?: string,
  ) {
    return this.historyService.findAll(
      sessionId,
      Math.min(Number(limit) || 50, 200),
      Number(offset) || 0,
    );
  }

  @Delete()
  @HttpCode(200)
  @ApiOperation({ summary: 'Clear listening history' })
  clear(@SessionId() sessionId: string) {
    return this.historyService.clear(sessionId);
  }
}
