import { Column, CreateDateColumn, Entity, Index, PrimaryGeneratedColumn } from 'typeorm';

@Entity('indexed_tracks')
export class IndexedTrack {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Index({ unique: true })
  @Column({ name: 'sc_track_id' })
  scTrackId: string;

  @Column({ nullable: true })
  title: string;

  @Column({ nullable: true })
  genre: string;

  @Column('text', { array: true, nullable: true })
  tags: string[];

  @Column({ name: 'duration_ms', nullable: true })
  durationMs: number;

  @Column({ name: 'artwork_url', nullable: true })
  artworkUrl: string;

  @Column({ name: 'stream_url', nullable: true })
  streamUrl: string;

  @Column({ name: 'raw_sc_data', type: 'jsonb', nullable: true })
  rawScData: Record<string, unknown>;

  @Column({ name: 'indexed_at', type: 'timestamptz', nullable: true })
  indexedAt: Date | null;

  @Index()
  @Column({ type: 'varchar', length: 8, nullable: true })
  language: string | null;

  @Column({ name: 'language_confidence', type: 'real', nullable: true })
  languageConfidence: number | null;

  @Index()
  @Column({ name: 's3_verified_at', type: 'timestamptz', nullable: true })
  s3VerifiedAt: Date | null;

  @Index()
  @Column({ name: 's3_missing_at', type: 'timestamptz', nullable: true })
  s3MissingAt: Date | null;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;
}
