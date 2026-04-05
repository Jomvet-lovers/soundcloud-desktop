import {
  BeforeInsert,
  Column,
  CreateDateColumn,
  Entity,
  Index,
  PrimaryColumn,
  UpdateDateColumn,
} from 'typeorm';
import { v7 as uuidv7 } from 'uuid';

export type CdnTrackQuality = 'hq' | 'sq';
export type CdnTrackStatus = 'pending' | 'ok' | 'error';

@Entity('cdn_tracks')
@Index(['trackUrn', 'quality'], { unique: true })
export class CdnTrack {
  @PrimaryColumn('uuid')
  id: string;

  @BeforeInsert()
  generateId() {
    if (!this.id) {
      this.id = uuidv7();
    }
  }

  @Index()
  @Column()
  trackUrn: string;

  @Column()
  quality: CdnTrackQuality;

  @Column({ type: 'text', nullable: true })
  cdnPath: string | null;

  @Index()
  @Column({ default: 'pending' })
  status: CdnTrackStatus;

  @Index()
  @Column({ type: 'timestamptz', nullable: true, default: () => 'NOW()' })
  lastAccessedAt: Date | null;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
