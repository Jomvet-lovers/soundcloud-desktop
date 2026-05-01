import { Body, Controller, Post } from '@nestjs/common';
import { ApiOperation, ApiTags } from '@nestjs/swagger';
import { LtrTrainerService } from './ltr-trainer.service.js';

@ApiTags('admin')
@Controller('admin/ltr')
export class LtrAdminController {
  constructor(private readonly trainer: LtrTrainerService) {}

  @Post('train')
  @ApiOperation({ summary: 'Manually trigger LTR (LightGBM ranker) training' })
  async train() {
    return this.trainer.trainNow();
  }
}
