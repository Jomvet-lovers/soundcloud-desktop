import { Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { LocalLikesController } from './local-likes.controller.js';
import { LocalLikesService } from './local-likes.service.js';

@Module({
  imports: [AuthModule],
  controllers: [LocalLikesController],
  providers: [LocalLikesService],
  exports: [LocalLikesService],
})
export class LocalLikesModule {}
