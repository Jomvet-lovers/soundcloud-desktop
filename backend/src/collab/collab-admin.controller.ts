import { Body, Controller, Get, Post } from '@nestjs/common';
import { ApiOperation, ApiTags } from '@nestjs/swagger';
import { CollabTrainerService } from './collab-trainer.service.js';
import { CollabVectorService } from './collab-vector.service.js';

@ApiTags('admin')
@Controller('admin/collab')
export class CollabAdminController {
  constructor(
    private readonly trainer: CollabTrainerService,
    private readonly vector: CollabVectorService,
  ) {}

  @Get('status')
  @ApiOperation({ summary: 'Collab status: collection dim, last train' })
  async status() {
    const dim = await this.vector.getCollabDim();
    return {
      collection_exists: dim !== null,
      dim,
    };
  }

  @Post('train')
  @ApiOperation({ summary: 'Manually trigger item2vec training on user sessions' })
  async train(@Body() body?: { dim?: number; minCount?: number }) {
    return this.trainer.trainNow(body ?? {});
  }
}
