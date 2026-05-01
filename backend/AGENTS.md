# Backend (NestJS BFF)

## Стек

- **NestJS 11** — фреймворк
- **Drizzle ORM** + `node-postgres` — БД (PostgreSQL). НЕ TypeORM.
- **drizzle-kit** — генерация миграций.
- **ioredis** — кеш + cron-leader lock.
- **JetStream (NATS)** — durable очереди (см. `bus/nats.service.ts`).
- **Fastify** через `@nestjs/platform-fastify`. Слушает UDS из `BACKEND_SOCKET` (или TCP `PORT` в dev).
- **pnpm** — пакетный менеджер.
- **Biome** — линтер (НЕ ESLint). Настройки: `useImportType: off`, `unsafeParameterDecoratorsEnabled: true`.

## Архитектура

- **BFF паттерн**: проксирует все вызовы к SoundCloud API.
- **Auth**: OAuth 2.1 + PKCE, сессии в PostgreSQL. Аутентификация через `x-session-id` header.
- **Stream proxy**: `GET /tracks/:id/stream?format=http_mp3_128` — проксирует аудио с поддержкой Range headers.
- **OpenAPI**: `/openapi.json`, Swagger UI — `/api`.
- **Gateway (Rust pid-1)**: TLS-терминатор + reverse-proxy + supervisor для N Node-воркеров. Бекенд бежит plain HTTP за UDS, gateway раздаёт по P2C least-loaded. См. `backend/gateway/`.
- **Cron leader**: `CronLeaderService` (`common/cron-leader/`) держит Redis lock `scd:cron:leader` (TTL 30s, renew 10s). Только лидер запускает зарегистрированные `@Cron`-джобы. При падении лидера — другой воркер автоматически перехватывает.
- **Модули**: auth, me, tracks, playlists, users, likes, reposts, resolve, health.

## Drizzle ORM

### Где что

- `src/db/schema.ts` — все таблицы в одном файле. `pgTable(...)`, snake_case колонки в БД, camelCase свойства в TS. Типы записей выводятся через `$inferSelect` / `$inferInsert` и реэкспортируются: `Session`, `NewSession` и т.д.
- `src/db/db.module.ts` — Global module, провайдит токены `DB` (`NodePgDatabase<typeof schema>`) и `PG_POOL` (`pg.Pool`). На старте запускает миграции под `pg_advisory_lock` — параллельные воркеры безопасно сериализуются, лок отпускается даже при фейле.
- `src/db/db.constants.ts` — symbols `DB`, `PG_POOL`.
- `drizzle.config.ts` — конфиг для `drizzle-kit` (TS).
- `drizzle/` — сгенерированные SQL миграции (`0000_*.sql`, `0001_*.sql`, ...). Коммитятся в репо. В Docker копируются в runtime образ.

### Как пользоваться в коде

```ts
import { Inject, Injectable } from '@nestjs/common';
import { eq, and, desc, inArray } from 'drizzle-orm';
import { DB } from '../db/db.constants.js';
import type { Database } from '../db/db.module.js';
import { sessions } from '../db/schema.js';

@Injectable()
export class FooService {
  constructor(@Inject(DB) private readonly db: Database) {}

  async byId(id: string) {
    return this.db.query.sessions.findFirst({ where: eq(sessions.id, id) });
  }

  async insert(data: NewSession) {
    const [row] = await this.db.insert(sessions).values(data).returning();
    return row;
  }
}
```

### Шпаргалка по идиомам

- **Получить одну запись**: `db.query.<table>.findFirst({ where: ..., columns: { id: true } })`.
- **Выборка с join'ами**: `db.select().from(a).leftJoin(b, eq(b.x, a.x)).where(...)`.
- **INSERT с возвратом**: `db.insert(t).values(v).returning()` — возвращает массив, [0] — первая строка.
- **UPSERT**: `db.insert(t).values(v).onConflictDoUpdate({ target: t.col, set: { foo: sql\`excluded.foo\` } })` — `excluded.*` ссылается на пытавшиеся вставиться значения.
- **Игнор дубликатов**: `.onConflictDoNothing({ target: t.col })`.
- **`now()` в SQL**: `set({ updatedAt: sql\`now()\` })` — drizzle сам кастует. Для default используй `.defaultNow()` или `.$onUpdate(() => new Date())`.
- **`FOR UPDATE SKIP LOCKED`**: `.for('update', { skipLocked: true })` (см. `oauth-apps.service.ts`).
- **`ORDER BY ... NULLS FIRST`**: `.orderBy(sql\`${col} ASC NULLS FIRST\`)`.
- **`COUNT(*) >= N` в HAVING**: `.having(sql\`COUNT(*) >= ${n}\`)`.
- **`RANDOM()`**: `.orderBy(sql\`RANDOM()\`)`.
- **JSONB**: `jsonb('col').$type<MyType>()` для типизации payload.
- **UUID v7 PK** (time-ordered, лучше для индексов): `uuid('id').primaryKey().$defaultFn(uuidv7)`. Для случайного UUID используй `.defaultRandom()` (= `gen_random_uuid()`).
- **`updatedAt` авто-обновляется на UPDATE**: `.$onUpdate(() => new Date())`.

### Workflow при изменении схемы

1. Правишь `src/db/schema.ts`.
2. Запускаешь `pnpm db:generate` (нужны env'ы `DATABASE_HOST` и т.д., смотри `drizzle.config.ts`). Создаётся новый файл `drizzle/000N_*.sql` + обновляется `drizzle/meta/_journal.json`.
3. **Открываешь сгенерированный SQL и читаешь его.** drizzle-kit может предложить `DROP TABLE` / `DROP COLUMN` если ты переименовал что-то — перепиши руками на `ALTER TABLE ... RENAME COLUMN` чтобы не терять данные.
4. Коммитишь schema + миграцию + `meta/`.
5. Деплоишь — на старте бекенда миграция применится автоматически (см. ниже).

### Скрипты в `package.json`

- `pnpm db:generate` — создать миграцию из diff между schema и текущим состоянием БД.
- `pnpm db:migrate` — применить вручную (обычно не нужно, см. ниже про прод).
- `pnpm db:push` — push schema без миграции (только для одноразовых dev-экспериментов, **на проде не использовать**).
- `pnpm db:drop` — удалить миграционный файл (помогает откатить неудачную генерацию).
- `pnpm db:studio` — веб-UI для просмотра данных.

### Как это работает на проде

При старте бекенда `DbModule.useFactory` для `DB` делает:

1. Берёт коннекшен из пула.
2. `SELECT pg_advisory_lock($1)` с фиксированным ключом `0x5cdb001`. Все остальные воркеры этого же бекенда блокируются на этом вызове.
3. Получивший лок воркер запускает `migrate()` из `drizzle-orm/node-postgres/migrator`. Он смотрит таблицу `__drizzle_migrations`, применяет недостающие, обновляет журнал.
4. `pg_advisory_unlock` + `client.release()`.
5. Остальные воркеры разблокируются, делают свой `migrate()` — он видит что всё применено, мгновенно выходит.

То есть **никаких ручных шагов на проде не нужно**. Поднял новую версию контейнера — миграции применятся сами, безопасно для N воркеров.

Управляющие env'ы:

- `SKIP_MIGRATIONS=1` — пропустить миграции на старте. Использовать когда деплоишь экстренно и не хочешь ждать долгую миграцию (потом применить руками через `pnpm db:migrate`).
- `DRIZZLE_MIGRATIONS_DIR=/path` — переопределить путь до папки `drizzle/` (по умолчанию `process.cwd()/drizzle`).
- `PG_POOL_MAX` — размер pg pool на воркер (default 10). Помни: умножается на число Node-воркеров (`BACKEND_WORKERS` у gateway). Не превышай `max_connections` postgres'а.

### Что делать если миграция упала на проде

1. Логи покажут конкретный SQL который сфейлился. **Не паниковать**: advisory lock освобождён через `finally`, БД не залочена.
2. Скорее всего бекенд в crash-loop'е (gateway рестартит). Останови контейнер: `docker compose stop backend`.
3. Воспроизведи проблему локально на копии prod-схемы или зайди на pg и посмотри состояние.
4. Если миграция зашла частично (`__drizzle_migrations` показывает что она применена, но не все DDL прошли) — это бывает с многошаговыми изменениями без транзакций. Чинить руками.
5. Подними бекенд с `SKIP_MIGRATIONS=1`, накати исправления отдельно через `pnpm db:migrate` либо ручным SQL, потом сними флаг и дай ему мигрировать дальше.

### Не используй

- `synchronize` (это TypeORM-специфичное, его больше нет).
- `db:push` на проде — он не пишет файл миграции, и состояние схемы перестанет соответствовать репо.
- Ручные `INSERT INTO __drizzle_migrations` для fake-применения — почти никогда не нужно. Если очень надо (миграция уже выполнена руками вне drizzle), вставляй запись с правильным `hash` из `meta/_journal.json` и `created_at`.

## Правила (общие)

- **Использовать декораторы NestJS** (@Controller, @Injectable, @Get и тд). НЕ писать роутинг вручную.
- **Drizzle** для работы с БД. Сырой SQL — только через `db.execute(sql\`...\`)` и только когда query builder реально не справляется.
- **class-validator + class-transformer** для валидации DTO. НЕ валидировать вручную.
- **ConfigService** для конфигурации. НЕ читать process.env напрямую в сервисах (исключения — db-bootstrap, supervisor-related env'ы).
- **HttpModule (axios)** для запросов к SoundCloud API. НЕ использовать node-fetch или свой HTTP клиент.
- **Ошибки**: бросать NestJS exceptions (NotFoundException, BadRequestException и тд). НЕ возвращать ошибки в body с 200 статусом.
- **Guard'ы** для аутентификации. НЕ проверять сессию внутри каждого контроллера вручную.
- **Cron-методы** (`@Cron`, `@Interval`) пиши обычно — `CronLeaderService` сам решит, запускать или нет на этом воркере.
- **NATS consumer'ы** регистрируй с фиксированным `durable_name` через `nats.consume(stream, durable, handler, filter)` — JetStream сам распределит сообщения round-robin между N воркерами.
- **Docker**: multi-stage Dockerfile (`backend/Dockerfile`), gateway (Rust) + Node в одном образе, gateway — pid-1. EXPOSE только 80/443.

## Проверки

- `npx tsc --noEmit` — типы
- `npx biome check` — линтинг
- `pnpm build` — продакшен-сборка через `nest build`
