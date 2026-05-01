import { resolve } from 'node:path';
import { Global, Inject, Logger, Module, OnModuleDestroy } from '@nestjs/common';
import { ConfigService } from '@nestjs/config';
import { drizzle, NodePgDatabase } from 'drizzle-orm/node-postgres';
import { migrate } from 'drizzle-orm/node-postgres/migrator';
import { Pool } from 'pg';
import { DB, PG_POOL } from './db.constants.js';
import * as schema from './schema.js';

export type Database = NodePgDatabase<typeof schema>;

const MIGRATION_LOCK_KEY = 0x5_c_d_d_b_001;

async function runMigrationsLocked(pool: Pool, db: Database): Promise<void> {
  const log = new Logger('Drizzle');
  const migrationsFolder = process.env.DRIZZLE_MIGRATIONS_DIR ?? resolve(process.cwd(), 'drizzle');
  const client = await pool.connect();
  try {
    await client.query('SELECT pg_advisory_lock($1)', [MIGRATION_LOCK_KEY]);
    log.log(`running migrations from ${migrationsFolder}`);
    await migrate(db, { migrationsFolder });
    log.log('migrations applied');
  } finally {
    try {
      await client.query('SELECT pg_advisory_unlock($1)', [MIGRATION_LOCK_KEY]);
    } finally {
      client.release();
    }
  }
}

@Global()
@Module({
  providers: [
    {
      provide: PG_POOL,
      inject: [ConfigService],
      useFactory: (config: ConfigService): Pool =>
        new Pool({
          host: config.get<string>('database.host'),
          port: config.get<number>('database.port'),
          user: config.get<string>('database.username'),
          password: config.get<string>('database.password'),
          database: config.get<string>('database.name'),
          max: Number.parseInt(process.env.PG_POOL_MAX ?? '10', 10),
          idleTimeoutMillis: 30_000,
        }),
    },
    {
      provide: DB,
      inject: [PG_POOL],
      useFactory: async (pool: Pool): Promise<Database> => {
        const db = drizzle(pool, { schema });
        if (process.env.SKIP_MIGRATIONS !== '1') {
          await runMigrationsLocked(pool, db);
        }
        return db;
      },
    },
  ],
  exports: [DB, PG_POOL],
})
export class DbModule implements OnModuleDestroy {
  constructor(@Inject(PG_POOL) private readonly pool: Pool) {}

  async onModuleDestroy(): Promise<void> {
    await this.pool.end();
  }
}
