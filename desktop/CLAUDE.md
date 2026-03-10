# Desktop (Tauri + React)

## Стек

- **Tauri v2** — нативная оболочка, Rust backend
- **React 19** + Vite — фронтенд
- **Tailwind CSS 4** — стили
- **Zustand** — стейт-менеджмент
- **TanStack Query** — серверный стейт, кеширование, пагинация
- **React Router 7** — роутинг
- **Radix UI** — примитивы (Slider, Dialog и тд)
- **Howler.js** — аудио движок
- **Biome** — линтер + форматтер (НЕ ESLint/Prettier)
- **pnpm** — пакетный менеджер

## Структура

```
desktop/
  src/
    components/    # React компоненты
      layout/      # AppShell, NowPlayingBar, Sidebar, Titlebar
      music/       # TrackCard, PlaylistCard, и тд
      ui/          # Skeleton, HorizontalScroll, общие UI
    pages/         # Home, Library, Search, TrackPage, UserPage, PlaylistPage
    stores/        # Zustand stores (player.ts, auth.ts)
    lib/           # Утилиты (audio.ts, api.ts, cache.ts, hooks.ts, cdn.ts)
  src-tauri/
    src/           # Rust: audio_server, proxy_server, proxy, discord, tray
    capabilities/  # Tauri permissions (default.json)
```

## Правила для React

- **React.memo** на все компоненты, которые могут ре-рендериться без причины.
- **Изолированные подписки.** Каждый компонент подписывается только на нужные поля через Zustand selectors: `usePlayerStore((s) => s.isPlaying)`, а не `usePlayerStore()`.
- **60fps анимации через DOM refs**, НЕ через React state. Пример: ProgressSlider обновляет `ref.style.left` в `subscribe()` listener, React не перерендеривается.
- **useSyncExternalStore** — для аудио-стейта (currentTime, duration). Snapshot функция должна возвращать стабильное значение (напр. `Math.floor()` для секунд), иначе 60 ре-рендеров/сек.
- **TanStack Query**: использовать `staleTime`, `setQueriesData` для optimistic updates, `invalidateQueries` с задержкой если API eventual consistent.
- **useCallback/useMemo** — только где реально нужно (тяжёлые вычисления, пропсы в memo-компоненты). Не на каждую функцию.

## Правила для Tauri (Rust)

- **Warp** для HTTP серверов. НЕ менять на actix/axum — warp уже async на tokio, конкурентный из коробки.
- **reqwest** для HTTP клиента. НЕ писать свой HTTP клиент.
- **tokio** рантайм. НЕ использовать std::thread для I/O. Блокирующие операции — через `tokio::spawn_blocking`.
- **Кеширование в прокси**: cacheable GET-ответы (image/*, font/*, css, js без no-store/no-cache) сохраняются в `{cache_dir}/assets/`. Ключ — SHA256(url). Запись на диск — `tokio::spawn`, не блокировать ответ.
- **Audio-сервер**: раздаёт MP3 из `{cache_dir}/audio/` с поддержкой Range requests. Читать файлы через `tokio::fs`, не `std::fs`.
- **`#[cfg(not(dev))]`** для localhost plugin и navigate. В dev — Vite devUrl.
- **Не буферизовать** большие ответы целиком если не нужно кешировать — стримить через `Body::wrap_stream`.
- **Ошибки**: возвращать HTTP-статусы (502, 400, 404), НЕ паниковать. `.unwrap()` допустим только для заведомо валидных операций (builder patterns).
- **Проверка**: `cargo check` после каждого изменения в Rust.

## Проверки

- `npx tsc --noEmit` — типы React/TS
- `cargo check` — компиляция Rust
- `npx biome check` — линтинг