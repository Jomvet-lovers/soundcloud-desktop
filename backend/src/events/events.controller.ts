import { Body, Controller, Post, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { EventsService } from './events.service.js';

class RecordEventDto {
  scUserId: string;
  scTrackId: string;
  eventType: string;
}

@ApiTags('events')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('events')
export class EventsController {
  constructor(private readonly events: EventsService) {}

  @Post()
  @ApiOperation({ summary: 'Record a user event (like, skip, play, etc.)' })
  async record(@Body() body: RecordEventDto) {
    await this.events.record(body.scUserId, body.scTrackId, body.eventType);
    return { ok: true };
  }
}
