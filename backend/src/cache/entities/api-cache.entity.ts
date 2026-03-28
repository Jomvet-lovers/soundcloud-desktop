import { Column, CreateDateColumn, Entity, Index, PrimaryColumn } from 'typeorm';

@Entity('api_cache')
export class ApiCache {
  /** SHA-256 хэш cache key */
  @PrimaryColumn('varchar', { length: 64 })
  key: string;

  @Column('jsonb')
  response: unknown;

  @Index()
  @Column('timestamptz')
  expiresAt: Date;

  @CreateDateColumn()
  createdAt: Date;
}
