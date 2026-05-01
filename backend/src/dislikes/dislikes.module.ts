import { forwardRef, Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { EventsModule } from '../events/events.module.js';
import { DislikesController } from './dislikes.controller.js';
import { DislikesService } from './dislikes.service.js';

@Module({
  imports: [AuthModule, forwardRef(() => EventsModule)],
  controllers: [DislikesController],
  providers: [DislikesService],
  exports: [DislikesService],
})
export class DislikesModule {}
