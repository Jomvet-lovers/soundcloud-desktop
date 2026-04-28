import { BeforeInsert, Column, CreateDateColumn, Entity, Index, PrimaryColumn } from 'typeorm';
import { v7 as uuidv7 } from 'uuid';

export type LinkRequestMode = 'pull' | 'push';
export type LinkRequestStatus = 'pending' | 'claimed' | 'failed';

/**
 * Одноразовый QR-токен для переноса сессии между устройствами.
 *
 * mode = 'pull' (десктоп без сессии генерит QR, телефон с сессией сканирует и пушит токены):
 *   - receiver создаёт LinkRequest без sourceSessionId, рендерит QR(claimToken)
 *   - source сканирует, делает claim передавая свой sessionId
 *   - бэк копирует SC-токены source-сессии в новую Session, сохраняет targetSessionId
 *   - receiver через poll status получает targetSessionId
 *
 * mode = 'push' (десктоп с сессией генерит QR, телефон без сессии сканирует и забирает):
 *   - sender создаёт LinkRequest с sourceSessionId, рендерит QR(claimToken)
 *   - receiver сканирует, делает claim без своего sessionId
 *   - бэк создаёт новую Session по токенам source, возвращает её id receiver-у
 */
@Entity('link_requests')
export class LinkRequest {
  @PrimaryColumn('uuid')
  id: string;

  @BeforeInsert()
  generateId() {
    if (!this.id) {
      this.id = uuidv7();
    }
  }

  /** Что выводится в QR. Random base64url, ~144 bits. */
  @Index({ unique: true })
  @Column()
  claimToken: string;

  @Column({ type: 'varchar' })
  mode: LinkRequestMode;

  /** Сессия, чьи SC-токены копируем. Для push заполняется при create, для pull — при claim. */
  @Column({ type: 'uuid', nullable: true })
  sourceSessionId: string | null;

  /** Сессия, в которую токены попали. Заполняется при claim. */
  @Column({ type: 'uuid', nullable: true })
  targetSessionId: string | null;

  @Column({ type: 'varchar', default: 'pending' })
  status: LinkRequestStatus;

  @Column({ type: 'varchar', nullable: true })
  error: string | null;

  @CreateDateColumn()
  createdAt: Date;

  @Column({ type: 'timestamp' })
  expiresAt: Date;
}
