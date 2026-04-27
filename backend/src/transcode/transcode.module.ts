import { HttpModule } from '@nestjs/axios';
import { Global, Module } from '@nestjs/common';
import { TranscodeTriggerService } from './transcode-trigger.service.js';

@Global()
@Module({
  imports: [HttpModule],
  providers: [TranscodeTriggerService],
  exports: [TranscodeTriggerService],
})
export class TranscodeModule {}
