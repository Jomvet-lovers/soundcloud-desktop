import { forwardRef, Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { EventsModule } from '../events/events.module.js';
import { DislikesController } from './dislikes.controller.js';
import { DislikesService } from './dislikes.service.js';
import { DislikedTrack } from './entities/disliked-track.entity.js';

@Module({
  imports: [TypeOrmModule.forFeature([DislikedTrack]), AuthModule, forwardRef(() => EventsModule)],
  controllers: [DislikesController],
  providers: [DislikesService],
  exports: [DislikesService],
})
export class DislikesModule {}
