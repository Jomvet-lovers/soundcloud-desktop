import { Column, CreateDateColumn, Entity, Index, PrimaryGeneratedColumn, Unique } from 'typeorm';

@Entity('disliked_tracks')
@Unique(['scUserId', 'scTrackId'])
export class DislikedTrack {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Index()
  @Column({ name: 'sc_user_id' })
  scUserId: string;

  @Index()
  @Column({ name: 'sc_track_id' })
  scTrackId: string;

  @Column({ name: 'track_data', type: 'jsonb', nullable: true })
  trackData: Record<string, unknown> | null;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;
}
