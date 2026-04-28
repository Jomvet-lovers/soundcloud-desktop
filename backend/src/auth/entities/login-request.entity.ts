import { BeforeInsert, Column, CreateDateColumn, Entity, Index, PrimaryColumn } from 'typeorm';
import { v7 as uuidv7 } from 'uuid';

export type LoginRequestStatus = 'pending' | 'processing' | 'completed' | 'failed';

/**
 * Pending OAuth-флоу. Живёт от /auth/login до /auth/callback (или TTL).
 * Изолирован от Session, чтобы long-lived сессии не загромождались
 * одноразовыми state/codeVerifier и чтобы переавторизация не перетирала
 * состояние существующей сессии до успеха.
 */
@Entity('login_requests')
export class LoginRequest {
  @PrimaryColumn('uuid')
  id: string;

  @BeforeInsert()
  generateId() {
    if (!this.id) {
      this.id = uuidv7();
    }
  }

  /** OAuth state — то что ушло в SoundCloud и вернётся в callback. */
  @Index({ unique: true })
  @Column()
  state: string;

  @Column()
  codeVerifier: string;

  @Column({ nullable: true })
  oauthAppId?: string;

  /** Если это re-auth, тут id существующей сессии. Иначе null — будет создана новая. */
  @Column({ type: 'uuid', nullable: true })
  targetSessionId: string | null;

  @Column({ type: 'varchar', default: 'pending' })
  status: LoginRequestStatus;

  /** После успешного callback — id сессии, которая в итоге авторизована. */
  @Column({ type: 'uuid', nullable: true })
  resultSessionId: string | null;

  @Column({ type: 'varchar', nullable: true })
  error: string | null;

  @CreateDateColumn()
  createdAt: Date;

  @Column({ type: 'timestamp' })
  expiresAt: Date;
}
