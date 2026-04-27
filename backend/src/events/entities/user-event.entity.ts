import { Column, CreateDateColumn, Entity, Index, PrimaryGeneratedColumn } from 'typeorm';

@Entity('user_events')
export class UserEvent {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Index()
  @Column({ name: 'sc_user_id' })
  scUserId: string;

  @Column({ name: 'sc_track_id' })
  scTrackId: string;

  @Column({ name: 'event_type' })
  eventType: string;

  @Column({ type: 'float' })
  weight: number;

  @Column({ default: false })
  seeded: boolean;

  @Index()
  @Column({ name: 'taste_applied_at', type: 'timestamptz', nullable: true })
  tasteAppliedAt: Date | null;

  @CreateDateColumn({ name: 'created_at' })
  createdAt: Date;
}
