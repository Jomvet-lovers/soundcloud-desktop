import { forwardRef, Module } from '@nestjs/common';
import { AuthModule } from '../auth/auth.module.js';
import { CollabModule } from '../collab/collab.module.js';
import { DislikesModule } from '../dislikes/dislikes.module.js';
import { IndexingModule } from '../indexing/indexing.module.js';
import { UserTasteModule } from '../user-taste/user-taste.module.js';
import { EventsController } from './events.controller.js';
import { EventsService } from './events.service.js';
import { IndexingQueueConsumer } from './indexing-queue.consumer.js';

@Module({
  imports: [
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
