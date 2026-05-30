# Worker (AI-слой)

## Назначение

**Только AI.** Воркер — это тонкий слой вокруг моделей: получил задачу из шины → прогнал через модель → отдал ответ обратно в шину.

Никаких HTTP endpoint'ов, ни входящих, ни исходящих к другим сервисам. Один API — **NATS**.

## Правила

- **Никакой бизнес-логики.** Воркер не знает про треки, плейлисты, юзеров, сессии. Не лазит в SoundCloud API. Не трогает
  PostgreSQL. **Не пишет в Qdrant** — считает вектор и отдаёт в шину, в Qdrant пишет только backend. Про индексированные
  треки знает только backend.
- **Только модели.** Загрузка при старте (`models.py`), инференс по задаче. Всё остальное — снаружи.
- **Никакого HTTP.** Воркеров может быть 1, 10, 100, 1000 — они все stateless и масштабируются горизонтально. Никто не знает их адреса, никаких ENV под каждый хост.
- **Коммуникация — NATS.**
  - Core NATS request-reply (`ai.rpc.*`) — короткие **синхронные** AI-задачи, где ждёт вызывающий (detect_language, search_queries, rank_lyrics, encode_text_mulan, resolve/verify/match, quality). Queue group `ai-workers`: один запрос = один воркер. Поздний ответ бесполезен → таймаут оправдан.
  - JetStream work queues (`INDEX_AUDIO`, `EMBED_LYRICS`, `TRANSCRIBE`, `TRAIN_*`) — тяжёлые **фоновые** durable задачи. Durable consumer, `ack_policy=explicit`, `ack_wait=30s`, `max_deliver=5`. Завершение → `done.*` publish.
  - `TRANSCRIBE` (self-gen лирика: demucs+whisper) — НЕ req/res: длительность не ограничена, синхронного клиента нет. Свой durable `transcribe-workers` и свой `WORKER_CONCURRENCY`-тег `transcribe`, чтобы тяжёлый GPU не вставал поперёк интерактивных `ai.rpc.*`.
- **Вход — готовые данные.** Текст в теле задачи. Аудио — ссылкой на S3/storage, один `GET` и всё. Воркер не знает про streaming-сервис, не знает про storage-логику.
- **Выход — только NATS.** Вектор едет в `done.*` payload (collab — блобом в Object Store, имя в сообщении); в Qdrant
  его кладёт backend, слушая `done.*`. Короткий ответ → NATS reply. Завершение → `done.*` publish.

## Lifecycle задачи (durable)

1. `fetch(1)` — один воркер берёт одну задачу за раз.
2. Запускается heartbeat `msg.in_progress()` каждые `TASK_HEARTBEAT_SEC` (10с) — сбрасывает `ack_wait` на стороне сервера.
3. Жёсткий таймаут `TASK_HARD_TIMEOUT_SEC` (2 мин; для тега `transcribe` — `TRANSCRIBE_HARD_TIMEOUT_SEC`, 30 мин). Если обработка дольше — `msg.nak(0)` → сразу другому воркеру.
4. Успех → `msg.ack()` → JetStream удаляет сообщение из WorkQueue.
5. Если воркер упал / рестарт / crash — heartbeat перестаёт идти, сервер через `ack_wait` переотправляет **другому** воркеру (до `max_deliver`).

## Файлы

Весь Python-код живёт в `src/`. Запуск — `python -m src.main`.

| Файл                         | Назначение                                                                                           |
|------------------------------|------------------------------------------------------------------------------------------------------|
| `src/main.py`                | entry point: connect NATS, load models, поднять лейны (runner.py), route AI subjects                 |
| `src/config.py`              | все env-переменные в одном месте                                                                     |
| `src/subjects.py`            | константы subjects/streams/durables (синхронизированы с `backend/src/bus/subjects.rs`)               |
| `src/bus/client.py`          | `connect()` к NATS                                                                                   |
| `src/bus/streams.py`         | `ensure_stream`, `ensure_consumer`                                                                   |
| `src/bus/rpc.py`             | `run_with_lifecycle` (JS work-queue) + `run_rpc_msg` (core-reply) + heartbeat                        |
| `src/models/device.py`       | `DEVICE`, `USE_FP16` (auto-detect CUDA/CPU)                                                          |
| `src/models/registry.py`     | `Models` dataclass + per-model asyncio locks                                                         |
| `src/models/loader.py`       | `load_all()` — MuQ/MuLan/bge-m3/xlm-roberta/Qwen/Whisper, fp16 на CUDA                               |
| `src/models/demucs.py`       | `ensure_demucs()` — ленивая загрузка (≈1.5 GB VRAM только при транскрипции)                          |
| `src/handlers/ai.py`         | detect_language, search_queries, rank_lyrics, encode_text_mulan                                      |
| `src/handlers/transcribe.py` | TRANSCRIBE: demucs (vocals) + Whisper → publish `done.transcribe` (пусто = self-gen-disable на бэке) |
| `src/runner.py`              | `run_batched_lane` (фан-аут+GPU) и `run_concurrent_lane` (N задач)                                   |
| `src/handlers/audio.py`      | INDEX_AUDIO: prepare (download+decode) → gpu_batch (MuQ+MuLan) → `done.index_audio`                  |
| `src/handlers/lyrics.py`     | EMBED_LYRICS: prepare → gpu_batch (bge-m3, батч) → `done.embed_lyrics`                               |
| `src/handlers/collab.py`     | TRAIN_COLLAB: gensim Word2Vec → вектора блобом в Object Store + `done.train_collab`                  |

## Лейны (runner.py)

Два раннера поверх durable pull-consumer:

- **`run_batched_lane`** (audio, lyrics): качалка (`prepare`, I/O) фанится на N задач
  (`WORKER_CONCURRENCY[tag]`) → очередь → один GPU-исполнитель (`gpu_batch`). Скачка
  перекрывает GPU; один владелец модели → локи на лейне не нужны.
  - lyrics батчится (bge-m3 сам маскирует паддинг).
  - audio — по одному треку на forward: MuQ маску из инпута не строит, у MuLan
    пулинг `mean(dim=-2)` без маски → паддинг разных длин испортил бы вектор.
- **`run_concurrent_lane`** (ai-rpc, transcribe, collab, quality): до N одновременных
  задач (fetch → spawn task, permit в его finally).

GPU-лок (`mulan_lock`/`lyrics_text_lock`) даётся батч-лейну только при `ai>0`, когда
ai-rpc шарит ту же модель; при `ai=0` локов нет.

## Масштабирование

Воркеры = горизонтально клонируемые stateless-контейнеры. NATS — единственная точка входа/выхода (Qdrant/PG/S3-запись —
на backend). Состояние задач (ack/pending/deliveries) хранит JetStream, не воркер.
