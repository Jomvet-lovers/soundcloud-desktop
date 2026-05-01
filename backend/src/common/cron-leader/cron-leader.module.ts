import { Module } from '@nestjs/common';
import { CronLeaderService } from './cron-leader.service.js';

@Module({
  providers: [CronLeaderService],
  exports: [CronLeaderService],
})
export class CronLeaderModule {}
