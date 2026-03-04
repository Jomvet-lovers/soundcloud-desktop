# SoundCloud Desktop v2 — Frontend Plan

## Концепция

Десктопное приложение для SoundCloud с фокусом на музыку. Минимум UI-шума, максимум контента. Glass-morphism, Vision Pro aesthetic — полупрозрачные панели, мягкие блюры, тонкие бордеры, плавные анимации. Тёмная тема по умолчанию.

**Стек:** Tauri 2 + React 19 + TypeScript + Vite
**Стилизация:** Tailwind CSS 4 + CSS custom properties для glass-эффектов
**State:** Zustand (лёгкий, без бойлерплейта)
**Запросы:** TanStack Query (кеширование, пагинация, инвалидация)
**Роутинг:** React Router 7
**Аудио:** HTML5 Audio API через кастомный хук (стрим через наш бэк `/tracks/:id/stream`)
**i18n:** react-i18next (русский + английский)

---

## Дизайн-система

### Цвета
```
--bg-primary:     rgba(10, 10, 12, 0.95)      # Почти чёрный фон
--bg-glass:       rgba(255, 255, 255, 0.04)    # Glass-панели
--bg-glass-hover: rgba(255, 255, 255, 0.08)    # Hover state
--bg-glass-active:rgba(255, 255, 255, 0.12)    # Active/pressed
--border-glass:   rgba(255, 255, 255, 0.08)    # Тонкие бордеры
--border-glass-hi:rgba(255, 255, 255, 0.15)    # Бордеры при hover
--text-primary:   rgba(255, 255, 255, 0.92)    # Основной текст
--text-secondary: rgba(255, 255, 255, 0.55)    # Вторичный
--text-tertiary:  rgba(255, 255, 255, 0.30)    # Подсказки
--accent:         #ff5500                       # SoundCloud orange
--accent-glow:    rgba(255, 85, 0, 0.15)       # Glow вокруг accent элементов
```

### Glass-компонент (базовый)
```css
.glass {
  background: var(--bg-glass);
  backdrop-filter: blur(40px) saturate(1.6);
  border: 1px solid var(--border-glass);
  border-radius: 16px;
}
```

### Типографика
- SF Pro Display / Inter / system fallback
- Заголовки: 600 weight, -0.02em letter-spacing
- Тело: 400 weight, line-height 1.5

### Анимации
- Все переходы: `200ms cubic-bezier(0.16, 1, 0.3, 1)` (Apple spring)
- Page transitions: slide + fade, 300ms
- Hover: scale(1.02) на карточках
- Skeleton loading с shimmer-эффектом

---

## Лейаут

```
┌─────────────────────────────────────────────────────┐
│ ▪▪▪  Titlebar (draggable, traffic lights)           │
├────────┬────────────────────────────────────────────┤
│        │                                            │
│  Side  │           Main Content                     │
│  bar   │           (scrollable)                     │
│        │                                            │
│  Nav   │                                            │
│  items │                                            │
│        │                                            │
│        │                                            │
│        │                                            │
├────────┴────────────────────────────────────────────┤
│  ▶ Now Playing Bar (always visible)                 │
│  [art] Title - Artist    ◀ ▶ ▶▶    ═══●════  3:21  │
└─────────────────────────────────────────────────────┘
```

### Sidebar (200px, glass)
- Logo / App name
- Nav links:
  - Home (feed)
  - Search
  - Library (playlists, likes)
  - Following
- Divider
- Your Playlists (list)
- Divider
- User avatar + username (bottom)

### Now Playing Bar (70px, glass, bottom-fixed)
- Track artwork (48x48, rounded-8)
- Title + Artist (truncated)
- Like button (heart)
- Controls: prev, play/pause, next
- Progress bar (thin, accent color, draggable)
- Current time / Duration
- Volume slider
- Queue button

---

## Страницы

### 1. Login
- Центрированная glass-карточка
- Лого SoundCloud Desktop
- Кнопка "Sign in with SoundCloud" (accent)
- Нажатие → `GET /auth/login` → открыть URL в системном браузере
- Поллинг `GET /auth/session` каждые 2с пока не authenticated
- Анимированный переход в приложение после логина

### 2. Home (Feed)
- `/me/feed` с cursor-пагинацией (infinite scroll)
- Карточки треков/плейлистов в ленте
- Track card: artwork, title, artist, play count, duration, play button overlay
- Секции: "Recent from following", "Your likes" (горизонтальный scroll)

### 3. Search
- Большой input с glass-стилем, автофокус
- Real-time debounced search (300ms)
- Табы: Tracks / Playlists / Users
- Результаты в grid/list
- Фильтры: genre, duration, bpm (collapsible)

### 4. Track Page
- Hero: большой artwork (blur background) + title + artist
- Play/Like/Repost кнопки
- Waveform визуализация (если есть waveform_url)
- Comments (timed, показываются на waveform)
- Related tracks (горизонтальный scroll)
- Track info: genre, tags, description

### 5. Playlist Page
- Hero: artwork + title + creator + track count + duration
- Play all / Shuffle кнопки
- Track list (sortable, numbered)
- Каждый трек: #, artwork mini, title, artist, duration, like, more menu

### 6. User Profile
- Header: avatar (large), username, bio, follower/following counts
- Follow/Unfollow кнопка
- Табы: Tracks / Playlists / Likes / Reposts
- Web profiles (links)

### 7. Library
- Табы: Liked Tracks / Liked Playlists / Your Playlists
- Grid view для плейлистов, list view для треков

### 8. Queue
- Slide-over panel (right side, glass)
- Now playing highlight
- Up next list (draggable reorder)
- Clear queue

---

## Компоненты

### Общие
- `GlassCard` — базовая glass-карточка
- `GlassButton` — кнопка с glass-стилем + варианты (primary/accent, ghost, icon)
- `Avatar` — круглый аватар с fallback
- `Skeleton` — shimmer loading placeholder
- `InfiniteScroll` — обёртка с intersection observer
- `Tooltip` — glass tooltip

### Музыка
- `TrackCard` — карточка трека (grid item)
- `TrackRow` — строка трека (list item)
- `PlaylistCard` — карточка плейлиста
- `UserCard` — карточка пользователя
- `Waveform` — визуализация формы волны
- `NowPlayingBar` — плеер внизу
- `QueuePanel` — панель очереди
- `VolumeSlider` — слайдер громкости
- `ProgressBar` — прогресс трека (draggable)

### Layout
- `AppShell` — основной лейаут (sidebar + content + player)
- `Sidebar` — навигация
- `Titlebar` — кастомный titlebar для Tauri (draggable)
- `PageHeader` — заголовок страницы с blur-фоном

---

## State Management (Zustand)

### Stores

```typescript
// Player store
interface PlayerStore {
  currentTrack: Track | null
  queue: Track[]
  isPlaying: boolean
  volume: number          // 0-1
  progress: number        // секунды
  duration: number
  shuffle: boolean
  repeat: 'off' | 'one' | 'all'

  play(track: Track): void
  pause(): void
  resume(): void
  next(): void
  prev(): void
  seek(seconds: number): void
  setVolume(v: number): void
  addToQueue(tracks: Track[]): void
  removeFromQueue(index: number): void
  clearQueue(): void
}

// Auth store
interface AuthStore {
  sessionId: string | null
  username: string | null
  isAuthenticated: boolean

  login(): Promise<void>
  logout(): Promise<void>
  checkSession(): Promise<void>
}
```

### TanStack Query — ключевые запросы
- `['me']` — профиль
- `['feed', cursor]` — лента
- `['tracks', 'search', query, filters]` — поиск треков
- `['track', trackUrn]` — детали трека
- `['track', trackUrn, 'comments']` — комменты
- `['playlist', playlistUrn]` — плейлист
- `['user', userUrn]` — пользователь
- `['me', 'likes', 'tracks']` — лайкнутые треки
- `['me', 'playlists']` — плейлисты

---

## Аудио

Стриминг через наш бэк:
```
GET /tracks/{trackUrn}/stream?format=http_mp3_128
Header: x-session-id: {sessionId}
```

Возвращает `audio/mpeg` напрямую. Поддерживает `Range` хедеры для seeking.

### usePlayer hook
```typescript
// Внутри — HTML5 Audio element
// audio.src = `http://localhost:3000/tracks/${urn}/stream?format=http_mp3_128`
// Кастомный хедер через fetch + createObjectURL или MediaSource
// Или: audio src с сессией в query param (если бэк поддержит)
```

**Важно:** `<audio>` не умеет слать кастомные хедеры. Варианты:
1. Бэк принимает session_id как query param для стрима: `/tracks/:id/stream?session_id=xxx&format=http_mp3_128`
2. Fetch + `URL.createObjectURL(blob)` — но не будет seeking
3. Fetch + `MediaSource API` — полный контроль, поддержка Range/seeking

**Рекомендация:** Вариант 1 (query param) — простейший и работает. Добавить поддержку в бэк.

---

## Tauri Integration

### Capabilities
- Window drag (titlebar)
- System tray с controls (play/pause/next)
- Media keys (play/pause/next/prev)
- Deep links (для OAuth callback, опционально)
- Window state persistence (size, position)

### Window Config (tauri.conf.json)
```json
{
  "windows": [{
    "title": "SoundCloud Desktop",
    "width": 1200,
    "height": 800,
    "minWidth": 900,
    "minHeight": 600,
    "decorations": false,
    "transparent": true
  }]
}
```

`decorations: false` + `transparent: true` — для кастомного titlebar и glass-эффекта на уровне окна.

---

## i18n (интернационализация)

**Библиотека:** `react-i18next` + `i18next`
**Языки:** Русский (`ru`), English (`en`)
**Дефолт:** определяется по системной локали, fallback — `en`
**Хранение выбора:** `localStorage` (через i18next-browser-languagedetector или zustand persist)

### Структура переводов

```
src/
└── i18n/
    ├── index.ts          # инициализация i18next
    └── locales/
        ├── en.json       # английские строки
        └── ru.json       # русские строки
```

### Namespace — единый файл на язык

```json
// en.json
{
  "nav": {
    "home": "Home",
    "search": "Search",
    "library": "Library",
    "following": "Following"
  },
  "player": {
    "queue": "Queue",
    "nowPlaying": "Now Playing",
    "upNext": "Up Next",
    "clearQueue": "Clear Queue",
    "shuffle": "Shuffle",
    "repeat": "Repeat"
  },
  "auth": {
    "signIn": "Sign in with SoundCloud",
    "signingIn": "Waiting for authorization...",
    "signOut": "Sign Out"
  },
  "search": {
    "placeholder": "Search tracks, playlists, users...",
    "tracks": "Tracks",
    "playlists": "Playlists",
    "users": "Users",
    "noResults": "No results found"
  },
  "track": {
    "comments": "Comments",
    "related": "Related Tracks",
    "addComment": "Add a comment...",
    "plays": "plays",
    "likes": "likes",
    "reposts": "reposts"
  },
  "playlist": {
    "playAll": "Play All",
    "shuffle": "Shuffle",
    "tracks_one": "{{count}} track",
    "tracks_other": "{{count}} tracks"
  },
  "user": {
    "followers": "Followers",
    "following": "Following",
    "tracks": "Tracks",
    "playlists": "Playlists",
    "likes": "Likes",
    "follow": "Follow",
    "unfollow": "Unfollow"
  },
  "library": {
    "likedTracks": "Liked Tracks",
    "likedPlaylists": "Liked Playlists",
    "yourPlaylists": "Your Playlists"
  },
  "common": {
    "loading": "Loading...",
    "error": "Something went wrong",
    "retry": "Retry",
    "seeAll": "See All",
    "language": "Language"
  },
  "settings": {
    "title": "Settings",
    "language": "Language",
    "languageEn": "English",
    "languageRu": "Русский"
  }
}
```

```json
// ru.json
{
  "nav": {
    "home": "Главная",
    "search": "Поиск",
    "library": "Библиотека",
    "following": "Подписки"
  },
  "player": {
    "queue": "Очередь",
    "nowPlaying": "Сейчас играет",
    "upNext": "Далее",
    "clearQueue": "Очистить очередь",
    "shuffle": "Перемешать",
    "repeat": "Повтор"
  },
  "auth": {
    "signIn": "Войти через SoundCloud",
    "signingIn": "Ожидание авторизации...",
    "signOut": "Выйти"
  },
  "search": {
    "placeholder": "Треки, плейлисты, пользователи...",
    "tracks": "Треки",
    "playlists": "Плейлисты",
    "users": "Пользователи",
    "noResults": "Ничего не найдено"
  },
  "track": {
    "comments": "Комментарии",
    "related": "Похожие треки",
    "addComment": "Написать комментарий...",
    "plays": "прослушиваний",
    "likes": "лайков",
    "reposts": "репостов"
  },
  "playlist": {
    "playAll": "Воспроизвести",
    "shuffle": "Перемешать",
    "tracks_one": "{{count}} трек",
    "tracks_few": "{{count}} трека",
    "tracks_many": "{{count}} треков"
  },
  "user": {
    "followers": "Подписчики",
    "following": "Подписки",
    "tracks": "Треки",
    "playlists": "Плейлисты",
    "likes": "Лайки",
    "follow": "Подписаться",
    "unfollow": "Отписаться"
  },
  "library": {
    "likedTracks": "Любимые треки",
    "likedPlaylists": "Любимые плейлисты",
    "yourPlaylists": "Ваши плейлисты"
  },
  "common": {
    "loading": "Загрузка...",
    "error": "Что-то пошло не так",
    "retry": "Повторить",
    "seeAll": "Показать все",
    "language": "Язык"
  },
  "settings": {
    "title": "Настройки",
    "language": "Язык",
    "languageEn": "English",
    "languageRu": "Русский"
  }
}
```

### Переключатель языка
В сайдбаре внизу (рядом с аватаром) или в settings popup — минималистичный тоггл `EN / RU`.

### Pluralization
Русский язык имеет 3 формы множественного числа (1 трек, 2 трека, 5 треков). i18next поддерживает это через суффиксы `_one`, `_few`, `_many`.

---

## Порядок реализации

### Phase 1: Скелет
1. Очистить шаблонный код
2. Установить зависимости (tailwind, zustand, tanstack-query, react-router, react-i18next, i18next)
3. Настроить Tailwind с кастомными токенами
4. Настроить i18n (react-i18next, локали en/ru, определение системной локали)
5. Создать дизайн-систему: GlassCard, GlassButton, Avatar, Skeleton
6. Создать AppShell layout (Sidebar + Content + NowPlayingBar)
7. Кастомный Titlebar для Tauri

### Phase 2: Авторизация
7. Auth store (zustand + persist)
8. Login page
9. API клиент (fetch wrapper с session_id)

### Phase 3: Плеер
10. Player store
11. usePlayer hook (audio element + state sync)
12. NowPlayingBar компонент
13. ProgressBar + VolumeSlider
14. Queue panel

### Phase 4: Основные страницы
15. Home (feed) с infinite scroll
16. Search (debounced, tabbed)
17. Library (likes, playlists)

### Phase 5: Детальные страницы
18. Track page (info, comments, related)
19. Playlist page (track list, play all)
20. User profile (tabs, follow)

### Phase 6: Polish
21. Анимации переходов между страницами
22. Skeleton loading states
23. Error states
24. Keyboard shortcuts (space=play/pause, arrows=seek, etc.)
25. Media keys через Tauri
26. System tray
