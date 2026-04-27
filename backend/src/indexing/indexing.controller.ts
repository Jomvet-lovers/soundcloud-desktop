import { Controller, Get, UseGuards } from '@nestjs/common';
import { ApiHeader, ApiOperation, ApiTags } from '@nestjs/swagger';
import { AuthGuard } from '../common/guards/auth.guard.js';
import { IndexingService } from './indexing.service.js';

@ApiTags('indexing')
@ApiHeader({ name: 'x-session-id', required: true })
@UseGuards(AuthGuard)
@Controller('indexing')
export class IndexingController {
  constructor(private readonly indexing: IndexingService) {}

  @Get('stats')
  @ApiOperation({ summary: 'Get SoundWave indexing progress' })
  async getStats() {
    return this.indexing.getStats();
  }
}
