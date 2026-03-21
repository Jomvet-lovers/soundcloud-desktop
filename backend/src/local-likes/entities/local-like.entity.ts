import { Column, CreateDateColumn, Entity, Index, PrimaryGeneratedColumn, Unique } from 'typeorm';

@Entity('local_likes')
@Unique(['soundcloudUserId', 'scTrackId'])
export class LocalLike {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Index()
  @Column()
  soundcloudUserId: string;

  @Column()
  scTrackId: string;

  @Column({ type: 'jsonb' })
  trackData: Record<string, unknown>;

  @CreateDateColumn()
  createdAt: Date;
}
