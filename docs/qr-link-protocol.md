# QR Cross-Device Login Protocol

Этот документ описывает протокол для мобильных клиентов SoundCloud Desktop. Если вы пишете
iOS/Android приложение, которое должно уметь логиниться через QR с десктопа (или наоборот),
используйте этот гайд.

> **TL;DR.** Один из двух девайсов показывает QR (одноразовый токен, TTL 5 минут), второй
> сканирует. Залогиненный девайс выступает _source_-ом и копирует свои SC-токены в новую
> сессию для второго девайса. Сессия второго девайса полностью независима — он сам
> обновляет токены, делает re-auth, может logout без влияния на источник.

---

## Архитектура

На бэке три сущности:

- **`Session`** — авторизованный юзер. У каждого девайса своя `Session` с уникальным `id`
  (UUID), которым он подписывает все запросы (`x-session-id` header). Внутри: SoundCloud
  `accessToken` / `refreshToken` / `expiresAt` / `scope`, `soundcloudUserId`, `username`,
  `oauthAppId`. Stable — `id` не меняется ни при re-auth, ни при refresh.
- **`LoginRequest`** — короткоживущий OAuth-флоу (state, codeVerifier). Используется
  для классического входа через SoundCloud OAuth. Мобильным клиентам не интересен.
- **`LinkRequest`** — одноразовый токен для переноса сессии между девайсами. Это то, что
  стоит за QR.

Каждый QR-токен содержит deep-link следующего вида:

```
scd://link?token=<claimToken>&mode=<pull|push>
```

- `claimToken` — random base64url, ~144 бит энтропии, индексирован уникально на бэке.
- `mode`:
  - `pull` — устройство, которое **показывает** QR, **не залогинено** и хочет получить
    сессию. Сканирующий девайс (источник, **залогинен**) пушит свои токены.
  - `push` — устройство, которое **показывает** QR, **залогинено** и хочет передать
    сессию. Сканирующий девайс (получатель, **не залогинен**) забирает токены.

В обе стороны итог один: на ранее не залогиненном девайсе появляется новая `Session` с
отдельным `sessionId`, токены которого изначально совпадают с source. После этого обе
сессии живут независимо.

---

## Endpoints

Базовый URL: тот же что и для остального API (см. `API_BASE` в десктоп-клиенте).
Все ответы — JSON. `x-session-id` header — UUID существующей `Session` (если есть).

### `POST /auth/link/create`

Создать LinkRequest. Кто его создаёт = тот, кто будет показывать QR.

**Headers:**
- `x-session-id` (string, opt) — обязателен для `mode=push`, запрещён для `mode=pull`.

**Body:** `{ "mode": "pull" | "push" }`

**Response (200):**
```json
{
  "linkRequestId": "uuid",
  "claimToken": "base64url string",
  "expiresAt": "2026-04-28T12:34:56.000Z"
}
```

Дальше показываем `scd://link?token=<claimToken>&mode=<mode>` в QR. Параллельно polling'ом
дёргаем `GET /auth/link/status?id=<linkRequestId>` раз в 2 секунды.

### `POST /auth/link/claim`

Вызывает девайс, который сканирует QR.

**Headers:**
- `x-session-id` (string, opt) — обязателен для `mode=pull`, не нужен для `mode=push`.

**Body:** `{ "claimToken": "..." }`

**Response (200):**
```json
{
  "sessionId": "uuid",
  "mode": "pull" | "push"
}
```

Поведение:
- **`mode=pull`** (caller — source, залогинен): бэк копирует токены caller'а в новую
  `Session` (target). Возвращает её `sessionId`. Caller для себя ничего не сохраняет —
  это просто "я подтвердил перенос". Target узнает свой `sessionId` через polling.
- **`mode=push`** (caller — target, не залогинен): бэк копирует токены source-сессии
  (которая прикреплена к LinkRequest при create) в новую `Session`. Возвращает её
  `sessionId`. **Caller сохраняет этот `sessionId` локально и считает себя залогиненным.**

**Ошибки:**
- `404` — токен невалидный (не существует или уже использован).
- `400` — токен уже использован/expired.
- `401` — caller прислал `x-session-id` от несуществующей сессии (для pull); или
  source-сессия требует re-auth (refresh не удался).

### `GET /auth/link/status?id=<linkRequestId>`

Polling.

**Response (200):**
```json
{
  "status": "pending" | "claimed" | "failed" | "expired",
  "mode": "pull" | "push",
  "sessionId": "uuid",
  "error": "..."
}
```

Поведение:
- **`pending`** — ещё никто не клеймил, ждём.
- **`claimed`** + `mode=pull` — поле `sessionId` содержит **новый** `sessionId` для
  устройства, которое показывало QR. Сохранить локально, считать себя залогиненным.
- **`claimed`** + `mode=push` — устройство-получатель уже забрало сессию. Showing
  device может закрыть оверлей, локально ничего сохранять не нужно (мы и так залогинены).
- **`failed`** / **`expired`** — пользователю показать кнопку "сгенерировать новый QR".

После `claimed` повторно `claim` вызвать нельзя (одноразовый токен).

### Стандартные auth endpoints (для общего сведения)

- `GET /auth/login` (header `x-session-id`, opt) — initiate classic OAuth flow.
  Возвращает `{ url, loginRequestId }`. Если передан валидный `x-session-id` — после
  callback токены запишутся в **ту же** Session (re-auth, sessionId не меняется).
- `GET /auth/login/status?id=<loginRequestId>` — polling для OAuth.
- `POST /auth/refresh` (header `x-session-id`) — обновить access token по refresh token.
- `GET /auth/session` (header `x-session-id`) — статус текущей сессии.
- `POST /auth/logout` (header `x-session-id`) — logout, удалить сессию.

---

## Юзкейсы

### A. Логиним мобильный с уже залогиненного десктопа (push)

1. На десктопе юзер открывает Settings → "Передать сессию на другое устройство".
2. Десктоп: `POST /auth/link/create` с `x-session-id` и `{ "mode": "push" }` → `claimToken`.
3. Десктоп показывает QR (`scd://link?token=...&mode=push`) и параллельно polling'ует
   `GET /auth/link/status?id=...`.
4. На мобильном юзер открывает приложение → "Войти по QR с другого устройства" → камера →
   сканирует QR.
5. Мобильный парсит deep-link, извлекает `claimToken`. Делает `POST /auth/link/claim` (без
   `x-session-id`!) с `{ claimToken }` → получает `{ sessionId, mode: "push" }`.
6. Мобильный сохраняет `sessionId` в storage и переходит в авторизованный режим.
7. Десктоп через polling видит `status=claimed`, закрывает оверлей.

### B. Логиним десктоп с уже залогиненного мобильного (pull)

1. На десктопе на login-screen юзер кликает "Войти по QR".
2. Десктоп: `POST /auth/link/create` (без `x-session-id`) с `{ "mode": "pull" }` →
   `claimToken`.
3. Десктоп показывает QR (`scd://link?token=...&mode=pull`) и polling'ует.
4. На мобильном (уже залогинен) юзер открывает приложение → "Логин на другом устройстве" →
   камера → сканирует QR.
5. Мобильный делает `POST /auth/link/claim` **с** `x-session-id` (своим) и
   `{ claimToken }` → получает `{ sessionId: <new_uuid>, mode: "pull" }`. Это `sessionId`
   нового устройства, мобильному он не нужен — просто индикатор успеха.
6. Десктоп через polling видит `status=claimed` + `sessionId=<new_uuid>` → сохраняет его
   локально, считает себя залогиненным.

### C. Цепочка устройств (важно для UX)

> "Сессию важно сохранять, чтоб потом опять туда сюда не бегать."

Этот сценарий из исходного ТЗ. После того как мобильный залогинился (по сценарию A или B),
он становится **полноценным source**: токены его сессии независимы (хоть и совпадают с
оригиналом в момент линка). Он может:

- Сам показывать QR в `mode=push`, чтобы залогинить, например, машину.
- Сам сканировать QR от ещё одного устройства в `mode=pull`.
- Сам делать `POST /auth/refresh`.
- Сам делать `POST /auth/logout` (это удалит **только его** сессию).

Чтобы цепочка работала, между линками не нужно перелогиниваться через SoundCloud.

> **Замечание про refresh tokens.** SoundCloud OAuth выдаёт один `refresh_token` на один
> auth code. Когда несколько сессий используют один и тот же `refresh_token` и кто-то из
> них рефрешит — SC может либо ротировать его (новые сессии получат 401 → re-auth), либо
> оставить старый валидным. Это поведение SC, мы не контролируем. На практике у двух-трёх
> устройств работает стабильно. Если получите 401 на refresh — просто пройдите OAuth
> заново на этом девайсе, sessionId сохранится.

---

## Безопасность

- `claimToken` — 24 байта random (~192 бит энтропии). Не угадать.
- TTL — 5 минут. После истечения LinkRequest помечается expired.
- Одноразовый: после `claim` статус становится `claimed`, повторный claim возвращает 400.
- В `mode=push` любой, кто получит `claimToken`, получит сессию — поэтому показывайте
  QR только в момент трансфера и закрывайте оверлей сразу после `claimed`.
- В `mode=pull` claim требует `x-session-id` от валидной сессии — это страховка от
  случайного claim'а. Перед claim'ом покажите юзеру `username` source-сессии (можно
  получить через `GET /auth/session` с своим `x-session-id`), чтобы он подтвердил, что
  передаёт правильный аккаунт.

---

## QR data-format checklist

- ✅ Deep-link `scd://link?token=<claimToken>&mode=<pull|push>` — поддерживайте
  registered URL scheme на iOS/Android, чтобы сканер открывал ваше приложение.
- ✅ Альтернативный fallback — `https://`-ссылка на лендинг, который универсальным
  редиректом отправит юзера в стор / в установленное приложение. Это можно добавить позже.
- ❌ **Не** кладите в QR `accessToken` или `sessionId` напрямую — только `claimToken`.
