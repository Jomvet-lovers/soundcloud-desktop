import { HttpModule } from '@nestjs/axios';
import { Module } from '@nestjs/common';
import { CdnService } from './cdn.service.js';

@Module({
  imports: [HttpModule],
  providers: [CdnService],
  exports: [CdnService],
})
export class CdnModule {}
