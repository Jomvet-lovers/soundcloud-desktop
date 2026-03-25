import { Controller, Get, HttpCode, Post, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOkResponse, ApiOperation, ApiTags } from '@nestjs/swagger';
import { SessionId } from '../common/decorators/session-id.decorator.js';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { PendingActionsService } from './pending-actions.service.js';

@ApiTags('pending-actions')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('pending-actions')
export class PendingActionsController {
  constructor(private readonly service: PendingActionsService) {}

  @Get()
  @ApiOperation({ summary: 'Get pending actions for current session' })
  list(@SessionId() sessionId: string) {
    return this.service.getForSession(sessionId);
  }

  @Get('stats')
  @ApiOperation({ summary: 'Get pending actions stats' })
  stats(@SessionId() sessionId: string) {
    return this.service.getStats(sessionId);
  }

  @Post('sync')
  @HttpCode(200)
  @ApiOperation({ summary: 'Manually trigger sync for current session' })
  sync(@SessionId() sessionId: string) {
    return this.service.syncForSession(sessionId);
  }
}
