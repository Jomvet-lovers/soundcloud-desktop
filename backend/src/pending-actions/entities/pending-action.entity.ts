import { v7 as uuidv7 } from 'uuid';
import {
  BeforeInsert,
  Column,
  CreateDateColumn,
  Entity,
  Index,
  PrimaryColumn,
  UpdateDateColumn,
} from 'typeorm';

export type ActionType =
  | 'like'
  | 'unlike'
  | 'repost'
  | 'unrepost'
  | 'comment'
  | 'playlist_create'
  | 'playlist_update'
  | 'playlist_delete'
  | 'like_playlist'
  | 'unlike_playlist'
  | 'repost_playlist'
  | 'unrepost_playlist';

export type ActionStatus = 'pending' | 'done' | 'failed';

@Entity('pending_actions')
export class PendingAction {
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
  sessionId: string;

  @Column()
  actionType: ActionType;

  /** URN цели действия (трек, плейлист) */
  @Column()
  targetUrn: string;

  /** Дополнительные данные (trackData для лайка, body для комментария и т.д.) */
  @Column({ type: 'jsonb', nullable: true })
  payload: Record<string, unknown> | null;

  @Column({ default: 'pending' })
  status: ActionStatus;

  @Column({ type: 'text', nullable: true })
  error: string | null;

  @Column({ default: 0 })
  retryCount: number;

  @CreateDateColumn()
  createdAt: Date;

  @UpdateDateColumn()
  updatedAt: Date;
}
