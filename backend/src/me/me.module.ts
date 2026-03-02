import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { SoundcloudModule } from '../soundcloud/soundcloud.module.js';
import { MeController } from './me.controller.js';
import { MeService } from './me.service.js';

@Module({
  imports: [SoundcloudModule, AuthModule],
  controllers: [MeController],
  providers: [MeService],
})
export class MeModule {}
