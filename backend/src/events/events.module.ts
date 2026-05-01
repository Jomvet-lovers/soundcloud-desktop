import { forwardRef, Module } from '@nestjs/common';
import { TypeOrmModule } from '@nestjs/typeorm';
import { AuthModule } from '../auth/auth.module.js';
import { CollabModule } from '../collab/collab.module.js';
import { DislikesModule } from '../dislikes/dislikes.module.js';
import { IndexingModule } from '../indexing/indexing.module.js';
import { UserTasteModule } from '../user-taste/user-taste.module.js';
import { UserEvent } from './entities/user-event.entity.js';
import { EventsController } from './events.controller.js';
import { EventsService } from './events.service.js';
import { IndexingQueueConsumer } from './indexing-queue.consumer.js';

@Module({
  imports: [
    TypeOrmModule.forFeature([UserEvent]),
    AuthModule,
    UserTasteModule,
    IndexingModule,
    CollabModule,
    forwardRef(() => DislikesModule),
  ],
  controllers: [EventsController],
  providers: [EventsService, IndexingQueueConsumer],
  exports: [EventsService],
})
export class EventsModule {}
