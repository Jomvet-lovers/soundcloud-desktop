import { v7 as uuidv7 } from 'uuid';
import {
  BeforeInsert,
  Column,
  CreateDateColumn,
  Entity,
  PrimaryColumn,
  UpdateDateColumn,
} from 'typeorm';

@Entity('oauth_apps')
export class OAuthApp {
  @PrimaryColumn('uuid')
  id: string;

  @BeforeInsert()
  generateId() {
    if (!this.id) {
      this.id = uuidv7();
    }
  }

  /** Человекочитаемое имя, напр. "app-main", "app-rotate-1" */
  @Column()
  name: string;

  @Column()
  clientId: string;

  @Column()
  clientSecret: string;

  @Column()
  redirectUri: string;

  /** Активна ли аппка (false = забанена или вручную выключена) */
  @Column({ default: true })
  active: boolean;

  /** Когда была забанена (null = не банилась) */
  @Column({ type: 'timestamptz', nullable: true })
  bannedAt: Date | null;

  /** Причина бана */
  @Column({ type: 'text', nullable: true })
  banReason: string | null;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
