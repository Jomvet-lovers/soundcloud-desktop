import { Injectable, Logger, OnModuleInit } from '@nestjs/common';
import { NatsService } from '../bus/nats.service.js';
import { STREAMS } from '../bus/subjects.js';
import { EventsService } from './events.service.js';

@Injectable()
export class IndexingQueueConsumer implements OnModuleInit {
  private readonly logger = new Logger(IndexingQueueConsumer.name);

  constructor(
    private readonly nats: NatsService,
    private readonly events: EventsService,
  ) {}

  async onModuleInit(): Promise<void> {
    await this.nats.consume(STREAMS.done.name, 'backend-events-done', async (data) => {
      const payload = data as { sc_track_id?: string | number };
      const scTrackId = payload.sc_track_id != null ? String(payload.sc_track_id) : '';
      if (!scTrackId) return;
      try {
        await this.events.applyPendingEventsForTrack(scTrackId);
      } catch (e) {
        this.logger.error(`apply pending ${scTrackId}: ${(e as Error).message}`);
      }
    });
    this.logger.log('Listening on PIPELINE_DONE stream');
  }
}
