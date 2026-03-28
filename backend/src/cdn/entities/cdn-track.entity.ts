import {
  BeforeInsert,
  Column,
  CreateDateColumn,
  Entity,
  PrimaryColumn,
  Unique,
  UpdateDateColumn,
} from 'typeorm';
import { v7 as uuidv7 } from 'uuid';

export enum CdnQuality {
  SQ = 'sq',
  HQ = 'hq',
}

export enum CdnStatus {
  PENDING = 'pending',
  OK = 'ok',
  ERROR = 'error',
}

@Entity('cdn_tracks')
@Unique(['trackUrn', 'quality'])
export class CdnTrack {
  @PrimaryColumn('uuid')
  id: string;

  @BeforeInsert()
  generateId() {
    if (!this.id) {
      this.id = uuidv7();
    }
  }

  @Column()
  trackUrn: string;

  @Column({ type: 'varchar', length: 2 })
  quality: CdnQuality;

  @Column({ nullable: true })
  cdnPath: string;

  @Column({ type: 'varchar', length: 10, default: CdnStatus.PENDING })
  status: CdnStatus;

  @Column({ type: 'boolean', nullable: true, default: null })
  hqAvailable: boolean | null;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
