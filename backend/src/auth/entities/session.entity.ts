import { v7 as uuidv7 } from 'uuid';
import {
  BeforeInsert,
  Column,
  CreateDateColumn,
  Entity,
  PrimaryColumn,
  UpdateDateColumn,
} from 'typeorm';

@Entity('sessions')
export class Session {
  @PrimaryColumn('uuid')
  id: string;

  @BeforeInsert()
  generateId() {
    if (!this.id) {
      this.id = uuidv7();
    }
  }

  @Column()
  accessToken: string;

  @Column()
  refreshToken: string;

  @Column()
  expiresAt: Date;

  @Column()
  scope: string;

  @Column({ nullable: true })
  soundcloudUserId: string;

  @Column({ nullable: true })
  username: string;

  @Column({ nullable: true })
  codeVerifier: string;

  @Column({ nullable: true })
  state: string;

  /** ID OAuth-аппки, через которую была авторизация */
  @Column({ nullable: true })
  oauthAppId: string;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
