# Worker (AI-слой)

## Назначение

**Только AI.** Воркер — это тонкий слой вокруг моделей: получил задачу из шины → прогнал через модель → отдал ответ обратно в шину.

Никаких HTTP endpoint'ов, ни входящих, ни исходящих к другим сервисам. Один API — **NATS**.

## Правила

- **Никакой бизнес-логики.** Воркер не знает про треки, плейлисты, юзеров, сессии. Не лазит в SoundCloud API. Не трогает PostgreSQL. Про индексированные треки знает только backend.
- **Только модели.** Загрузка при старте (`models.py`), инференс по задаче. Всё остальное — снаружи.
- **Никакого HTTP.** Воркеров может быть 1, 10, 100, 1000 — они все stateless и масштабируются горизонтально. Никто не знает их адреса, никаких ENV под каждый хост.
- **Коммуникация — NATS.**
  - Core NATS request-reply (`ai.rpc.*`) — короткие AI-задачи (detect_language, search_queries, rank_lyrics, transcribe). Queue group `ai-workers`: один запрос = один воркер.
  - JetStream work queues (`INDEX_AUDIO`, `EMBED_LYRICS`) — тяжёлые durable задачи. Durable consumer, `ack_policy=explicit`, `ack_wait=30s`, `max_deliver=5`.
- **Вход — готовые данные.** Текст в теле задачи. Аудио — ссылкой на S3/storage, один `GET` и всё. Воркер не знает про streaming-сервис, не знает про storage-логику.
- **Выход — ответ модели.** Вектор → Qdrant. Короткий ответ → NATS reply. Уведомление о завершении → `done.*` publish.

## Lifecycle задачи (durable)

1. `fetch(1)` — один воркер берёт одну задачу за раз.
2. Запускается heartbeat `msg.in_progress()` каждые `TASK_HEARTBEAT_SEC` (10с) — сбрасывает `ack_wait` на стороне сервера.
3. Жёсткий таймаут `TASK_HARD_TIMEOUT_SEC` (2 мин). Если обработка дольше — `msg.nak(0)` → сразу другому воркеру.
4. Успех → `msg.ack()` → JetStream удаляет сообщение из WorkQueue.
5. Если воркер упал / рестарт / crash — heartbeat перестаёт идти, сервер через `ack_wait` переотправляет **другому** воркеру (до `max_deliver`).

## Файлы

Весь Python-код живёт в `src/`. Запуск — `python -m src.main`.

| Файл | Назначение |
|------|-----------|
| `src/main.py` | entry point: connect NATS, load models, spawn pull-consumers, route AI subjects |
| `src/config.py` | все env-переменные в одном месте |
| `src/subjects.py` | константы subjects/streams/durables (синхронизированы с `backend/src/bus/subjects.ts`) |
| `src/bus/client.py` | `connect()` к NATS |
| `src/bus/streams.py` | `ensure_stream`, `ensure_consumer` |
| `src/bus/rpc.py` | `run_with_lifecycle` (JS work-queue) + `run_rpc_msg` (core-reply) + heartbeat |
| `src/models/device.py` | `DEVICE`, `USE_FP16` (auto-detect CUDA/CPU) |
| `src/models/registry.py` | `Models` dataclass + per-model asyncio locks |
| `src/models/loader.py` | `load_all()` — MuQ/MuLan/bge-m3/xlm-roberta/Qwen/Whisper, fp16 на CUDA |
| `src/models/demucs.py` | `ensure_demucs()` — ленивая загрузка (≈1.5 GB VRAM только при транскрипции) |
| `src/storage/qdrant.py` | Qdrant: upsert + idempotency check |
| `src/handlers/ai.py` | detect_language, search_queries, rank_lyrics, encode_text_mulan |
| `src/handlers/transcribe.py` | ai.transcribe — demucs (vocals) + Whisper |
| `src/handlers/audio.py` | INDEX_AUDIO: download → MuQ + MuQ-MuLan → Qdrant → publish `done.index_audio` |
| `src/handlers/lyrics.py` | EMBED_LYRICS: bge-m3 → Qdrant → publish `done.embed_lyrics` |

## Масштабирование

Воркеры = горизонтально клонируемые stateless-контейнеры. NATS — единственная точка входа/выхода. Qdrant — куда пишутся вектора. Состояние задач (ack/pending/deliveries) хранит JetStream, не воркер.
