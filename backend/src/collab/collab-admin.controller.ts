import { Body, Controller, Post } from '@nestjs/common';
import { ApiOperation, ApiTags } from '@nestjs/swagger';
import { CollabTrainerService } from './collab-trainer.service.js';

@ApiTags('admin')
@Controller('admin/collab')
export class CollabAdminController {
  constructor(private readonly trainer: CollabTrainerService) {}

  @Post('train')
  @ApiOperation({ summary: 'Manually trigger item2vec training on user sessions' })
  async train(@Body() body?: { dim?: number; minCount?: number }) {
    return this.trainer.trainNow(body ?? {});
  }
}
