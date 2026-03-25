# SoundCloud Desktop — Backend

BFF-сервер для десктопного приложения SoundCloud. Проксирует SoundCloud API, управляет OAuth 2.1 + PKCE авторизацией, хранит сессии в PostgreSQL.

## Стек

NestJS 11 · TypeORM · PostgreSQL 17 · Biome · Docker

---

## 1. Регистрация приложения в SoundCloud

1. Авторизуйся на [soundcloud.com](https://soundcloud.com)
2. Перейди на [soundcloud.com/you/apps](https://soundcloud.com/you/apps/)
3. Создай новое приложение
4. Получишь два секрета:

| Переменная | Откуда |
|---|---|
| `SOUNDCLOUD_CLIENT_ID` | Страница приложения, поле **Client ID** |
| `SOUNDCLOUD_CLIENT_SECRET` | Страница приложения, поле **Client Secret** |

5. Там же укажи **Redirect URI**:

```
http://localhost:3000/auth/callback
```

> Этот URL должен **точно совпадать** с тем, что указан в `.env` (`SOUNDCLOUD_REDIRECT_URI`).
> Для прода — заменить на реальный домен.

---

## 2. Настройка окружения

```bash
cp .env.example .env
```

Заполни `.env`:

```env
SOUNDCLOUD_CLIENT_ID=твой_client_id
SOUNDCLOUD_CLIENT_SECRET=твой_client_secret
SOUNDCLOUD_REDIRECT_URI=http://localhost:3000/auth/callback

DATABASE_HOST=localhost
DATABASE_PORT=5432
DATABASE_USERNAME=soundcloud
DATABASE_PASSWORD=soundcloud
DATABASE_NAME=soundcloud_desktop

PORT=3000
```

### Дополнительные переменные (опционально)

| Переменная | Описание |
|---|---|
| `SC_PROXY_URL` | CF Worker URL — все запросы к SC API пойдут через него с `X-Target` header |
| `CDN_BASE_URL` | SecureServe CDN URL для кэширования аудио |
| `CDN_AUTH_TOKEN` | Токен авторизации CDN |
| `TELEGRAM_BOT_TOKEN` | Бот для алертов при бане OAuth-аппки |
| `TELEGRAM_CHAT_ID` | Чат для Telegram алертов |
| `ADMIN_TOKEN` | Токен для Admin API (`/oauth-apps`) |

---

## 3. Запуск

### Docker (рекомендуется)

```bash
# Dev — с hot-reload, PG в контейнере
docker compose --profile dev up

# Prod — собранный образ + PG
docker compose --profile prod up

# Только база (если запускаешь сервер локально)
docker compose up db
```

### Локально

```bash
pnpm install
pnpm start:dev     # watch mode
pnpm start:debug   # с дебаггером
pnpm build && pnpm start:prod  # production
```

> Требуется запущенный PostgreSQL (см. настройки в `.env`).

---

## 4. Доступные URL

| URL | Описание |
|---|---|
| `http://localhost:3000/health` | Health check |
| `http://localhost:3000/api` | Swagger UI |
| `http://localhost:3000/openapi.json` | OpenAPI-спецификация |

---

## 5. Авторизация — как работает

```
Приложение                  Наш бэкенд                 SoundCloud
    │                           │                           │
    ├── GET /auth/login ───────►│                           │
    │◄── { url, sessionId } ────┤                           │
    │                           │                           │
    │   Юзер открывает url ─────┼──────────────────────────►│
    │                           │                           │
    │                           │◄── GET /auth/callback ────┤
    │                           │    ?code=...&state=...    │
    │                           │                           │
    │                           ├── POST /oauth/token ─────►│
    │                           │◄── access_token ──────────┤
    │                           │                           │
    │                           │  Показывает HTML-страницу │
    │                           │  "Успешно" / "Ошибка"     │
    │                           │                           │
    ├── GET /auth/session ─────►│  (по x-session-id)        │
    │◄── { authenticated: true }┤                           │
```

### Эндпоинты авторизации

| Метод | Путь | Описание |
|---|---|---|
| GET | `/auth/login` | Получить URL авторизации + sessionId |
| GET | `/auth/callback` | Колбэк от SoundCloud (автоматически) |
| GET | `/auth/session` | Статус сессии |
| POST | `/auth/refresh` | Обновить токен |
| POST | `/auth/logout` | Выйти |

Все остальные эндпоинты требуют хедер `x-session-id` с sessionId, полученным при логине.

---

## 6. API — покрытие SoundCloud

### Me (текущий пользователь)
`GET /me` · `/me/feed` · `/me/feed/tracks` · `/me/likes/tracks` · `/me/likes/playlists` · `/me/followings` · `/me/followings/tracks` · `/me/followers` · `/me/playlists` · `/me/tracks`
`PUT /me/followings/:userUrn` · `DELETE /me/followings/:userUrn`

### Tracks
`GET /tracks` — поиск · `GET /tracks/:id` · `PUT /tracks/:id` · `DELETE /tracks/:id`
`GET /tracks/:id/streams` · `/tracks/:id/comments` · `/tracks/:id/favoriters` · `/tracks/:id/reposters` · `/tracks/:id/related`
`POST /tracks/:id/comments`

### Playlists
`GET /playlists` — поиск · `POST /playlists` · `GET /playlists/:id` · `PUT /playlists/:id` · `DELETE /playlists/:id`
`GET /playlists/:id/tracks` · `/playlists/:id/reposters`

### Users
`GET /users` — поиск · `GET /users/:id` · `/users/:id/followers` · `/users/:id/followings` · `/users/:id/tracks` · `/users/:id/playlists` · `/users/:id/likes/tracks` · `/users/:id/likes/playlists` · `/users/:id/web-profiles`

### Likes
`POST /likes/tracks/:id` · `DELETE /likes/tracks/:id`
`POST /likes/playlists/:id` · `DELETE /likes/playlists/:id`
`GET /likes/playlists/:id` — проверка

### Reposts
`POST /reposts/tracks/:id` · `DELETE /reposts/tracks/:id`
`POST /reposts/playlists/:id` · `DELETE /reposts/playlists/:id`

### OAuth Apps (Admin)
`GET /oauth-apps` · `POST /oauth-apps` · `PATCH /oauth-apps/:id` · `DELETE /oauth-apps/:id`
> Требуют хедер `x-admin-token`.

### Pending Actions
`GET /pending-actions` — список ожидающих действий для сессии
`GET /pending-actions/stats` — `{ pending, failed }` счётчики
`POST /pending-actions/sync` — ручная синхронизация

### Resolve
`GET /resolve?url=...` — резолв SoundCloud URL в ресурс

---

## 7. Bypass-система

### Multi-app OAuth ротация
Пул OAuth-аппок в PostgreSQL (`oauth_apps`). При логине выбирается случайная активная аппка. При 403 CloudFront — аппка помечается забаненной, отправляется Telegram алерт, выбирается следующая.

Управление через Admin API (`/oauth-apps`) с токеном `ADMIN_TOKEN`.

Backward-compatible: если в БД нет аппок, автоматически создаётся одна из env-переменных `SOUNDCLOUD_CLIENT_ID` / `SOUNDCLOUD_CLIENT_SECRET`.

### Cloudflare proxy
Если задан `SC_PROXY_URL` — все запросы к SC API идут через CF Worker. Паттерн: `{SC_PROXY_URL}/{base64(originalUrl)}` с заголовком `X-Target: base64(originalUrl)`. Тот же CF Worker что и для `images.soundcloud.su` в Tauri-прокси.

### CDN аудио кэш
При запросе стрима: проверяется наличие на CDN (`HEAD`). Если есть — `302 Redirect`. Если нет — стримится с SC, параллельно заливается на CDN (fire-and-forget). Клиент ничего не знает о CDN.

### Очередь отложенных действий
При бане (403 CloudFront) действия пользователя (лайки, репосты, комменты, плейлисты) сохраняются в `pending_actions` (PostgreSQL). Background job каждые 60 сек пытается синхронизировать. Макс. 5 ретраев на действие.

---

## 8. Структура проекта

```
src/
├── main.ts                     # Bootstrap, Swagger setup
├── app.module.ts               # Root module
├── config/configuration.ts     # Env-based конфиг
├── auth/                       # OAuth 2.1 + PKCE, сессии
│   ├── entities/session.entity.ts
│   ├── callback-page.ts        # Glass UI страница колбэка
│   └── ...
├── soundcloud/                 # HTTP-клиент к SC API + типы
├── oauth-apps/                 # Пул OAuth-аппок, ротация, Telegram алерты
├── cdn/                        # CDN-интеграция (SecureServe)
├── pending-actions/            # Очередь отложенных действий
├── me/                         # /me/*
├── tracks/                     # /tracks/*
├── playlists/                  # /playlists/*
├── users/                      # /users/*
├── likes/                      # /likes/*
├── reposts/                    # /reposts/*
├── local-likes/                # Локальные лайки (fallback)
├── resolve/                    # /resolve
├── health/                     # /health
└── common/
    ├── guards/auth.guard.ts    # Проверка x-session-id
    ├── interceptors/           # BanDetectorInterceptor
    ├── decorators/             # @AccessToken(), @SessionId()
    └── dto/pagination.dto.ts   # Пагинация
```

---

## 9. Скрипты

```bash
pnpm start:dev      # Dev с hot-reload
pnpm build          # Сборка
pnpm start:prod     # Запуск собранного
pnpm lint           # Biome lint
pnpm format         # Biome format
pnpm check          # Biome lint + format
pnpm test           # Unit-тесты
pnpm test:e2e       # E2E-тесты
```
