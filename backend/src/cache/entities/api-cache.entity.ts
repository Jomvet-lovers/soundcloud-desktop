import { Column, CreateDateColumn, Entity, Index, PrimaryColumn } from 'typeorm';

@Entity('api_cache')
@Index(['cacheKey', 'scope', 'sessionId'])
export class ApiCache {
  /** SHA-256 хэш cache key */
  @PrimaryColumn('varchar', { length: 64 })
  key: string;

  @Column('jsonb')
  response: unknown;

  @Index()
  @Column('timestamptz')
  expiresAt: Date;

  @Column('varchar', { length: 128, nullable: true })
  cacheKey: string | null;

  @Column('varchar', { length: 16, nullable: true })
  scope: 'shared' | 'user' | null;

  @Column('varchar', { length: 64, nullable: true })
  sessionId: string | null;

  @CreateDateColumn()
  createdAt: Date;
}
