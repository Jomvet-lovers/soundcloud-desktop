import { Inject, Injectable, Logger, type OnModuleInit } from '@nestjs/common';
import { QdrantClient } from '@qdrant/js-client-rest';

@Injectable()
export class QdrantInitService implements OnModuleInit {
  private readonly logger = new Logger(QdrantInitService.name);

  constructor(
    @Inject('QDRANT_CLIENT')
    private readonly qdrant: QdrantClient,
  ) {}

  async onModuleInit() {
    try {
      const { collections } = await this.qdrant.getCollections();
      const existing = new Set(collections.map((c) => c.name));

      await this.ensureCollection(existing, 'tracks_mert', 1024);
      await this.ensureCollection(existing, 'tracks_clap', 512);
      await this.ensureCollection(existing, 'tracks_lyrics', 1024);
      await this.ensureCollection(existing, 'user_taste_mert', 1024);
      await this.ensureCollection(existing, 'user_taste_clap', 512);
      await this.ensureCollection(existing, 'user_taste_lyrics', 1024);
    } catch (e) {
      this.logger.warn(`Qdrant init skipped (not available): ${(e as Error).message}`);
    }
  }

  private async ensureCollection(existing: Set<string>, name: string, size: number) {
    if (existing.has(name)) return;

    await this.qdrant.createCollection(name, {
      vectors: { size, distance: 'Cosine' },
      on_disk_payload: true,
    });
    this.logger.log(`Created collection: ${name}`);
  }
}
