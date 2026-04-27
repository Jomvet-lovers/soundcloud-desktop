import { Column, CreateDateColumn, Entity, Index, PrimaryColumn } from 'typeorm';

export type LyricsSource =
  | 'lrclib'
  | 'musixmatch'
  | 'lyricsovh'
  | 'genius'
  | 'textyl'
  | 'self_gen'
  | 'none';

@Entity('lyrics_cache')
export class LyricsCache {
  @PrimaryColumn({ name: 'sc_track_id' })
  scTrackId: string;

  @Column({ name: 'synced_lrc', type: 'text', nullable: true })
  syncedLrc: string | null;

  @Column({ name: 'plain_text', type: 'text', nullable: true })
  plainText: string | null;

  @Column({ type: 'varchar', length: 16 })
  source: LyricsSource;

  @Index()
  @Column({ type: 'varchar', length: 8, nullable: true })
  language: string | null;

  @Column({ name: 'language_confidence', type: 'real', nullable: true })
  languageConfidence: number | null;

  @Column({ name: 'embedded_at', type: 'timestamptz', nullable: true })
  embeddedAt: Date | null;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;
}
