import { Column, CreateDateColumn, Entity, Index, PrimaryGeneratedColumn } from 'typeorm';

@Entity('listening_history')
export class ListeningHistory {
  @PrimaryGeneratedColumn('uuid')
  id: string;

  @Index()
  @Column()
  soundcloudUserId: string;

  @Column()
  scTrackId: string;

  @Column()
  title: string;

  @Column()
  artistName: string;

  @Column({ nullable: true })
  artworkUrl: string;

  @Column({ type: 'int' })
  duration: number;

  @CreateDateColumn()
  playedAt: Date;
}
