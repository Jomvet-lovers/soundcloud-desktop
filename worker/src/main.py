"""Воркер = только AI-слой. Shina — NATS (JetStream). HTTP/Redis не используются.

- Все задачи идут через JetStream pull-consumer'ы.
- Параллелизм управляется ENV `WORKER_CONCURRENCY` (см. config.py):
    - пусто / "1"          → один глобальный Semaphore(1) на все типы задач
                              (дефолт = старое поведение, GPU-friendly).
    - "N"                  → один глобальный Semaphore(N) на все типы.
    - "ai=4,audio=1,…"     → отдельный семафор на каждый тип (CPU-friendly:
                              лёгкие RPC не блокируются тяжёлым INDEX_AUDIO).
  Пока семафор занят, воркер НЕ делает fetch по этому типу — сообщения
  остаются в стриме и подхватываются другими воркерами (queue group через
  общий durable).
- Подтверждение "я работаю" раз в TASK_HEARTBEAT_SEC, жёсткий таймаут TASK_HARD_TIMEOUT_SEC.
"""
import asyncio
import logging
import os
import signal
import threading

from . import subjects as subj
from .bus import connect, ensure_consumer, run_rpc_msg, run_with_lifecycle
from .config import WORKER_CONCURRENCY
from .handlers import ai, audio, lyrics
from .handlers import collab as collab_handler
from .handlers import quality as quality_handler
from .handlers.resolve import match_track, resolve_artist, verify_existence
from .handlers.transcribe import transcribe
from .models import load_all
from .storage import ensure_collections, new_client

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
for noisy in ("httpx", "httpcore", "urllib3", "huggingface_hub", "filelock"):
    logging.getLogger(noisy).setLevel(logging.WARNING)
log = logging.getLogger(__name__)

TAGS = ("ai", "audio", "lyrics", "collab", "quality")


def _build_semaphores() -> dict[str, asyncio.Semaphore | None]:
    """Строит {tag: Semaphore | None} по WORKER_CONCURRENCY.

    None означает что тэг полностью отключён (`tag=0` в per-tag режиме):
    pull-loop не запускается, consumer не регистрируется, связанные модели
    не грузятся в load_all().
    """
    cfg = WORKER_CONCURRENCY
    if isinstance(cfg, int):
        shared = asyncio.Semaphore(cfg)
        log.info(f"WORKER_CONCURRENCY: global Semaphore({cfg}) shared across {list(TAGS)}")
        return {tag: shared for tag in TAGS}
    sems: dict[str, asyncio.Semaphore | None] = {}
    for tag in TAGS:
        n = cfg.get(tag, 1)
        sems[tag] = asyncio.Semaphore(n) if n > 0 else None
    log.info(
        "WORKER_CONCURRENCY per-tag: "
        + ", ".join(
            f"{t}={cfg.get(t, 1)}" + (" [DISABLED]" if sems[t] is None else "")
            for t in TAGS
        )
    )
    return sems


async def _js_pull_loop(
    js,
    sem: asyncio.Semaphore,
    stream: str,
    durable: str,
    subject: str,
    handler_factory,
    tag: str,
    stop: asyncio.Event,
    *,
    is_rpc: bool,
    nc=None,
) -> None:
    """Пока семафор занят — не вызываем fetch, сообщения достаются другим воркерам.

    После N подряд ошибок fetch (обычно — обрыв NATS) пересоздаём подписку,
    иначе зомби-psub будет вечно отдавать ошибки даже после реконнекта коннекта.
    """
    psub = await js.pull_subscribe(subject, durable=durable)
    log.info(f"JS pull-consumer started: {stream}/{durable} → {subject}")
    err_streak = 0

    while not stop.is_set():
        await sem.acquire()
        try:
            try:
                msgs = await psub.fetch(batch=1, timeout=1)
                err_streak = 0
            except asyncio.TimeoutError:
                sem.release()
                continue
            except asyncio.CancelledError:
                sem.release()
                raise
            except Exception as e:
                if stop.is_set():
                    sem.release()
                    return
                err_streak += 1
                log.error(f"{tag} fetch failed ({err_streak}): {e}")
                sem.release()
                if err_streak >= 5:
                    log.warning(f"{tag} resubscribing after {err_streak} fetch errors")
                    try:
                        await psub.unsubscribe()
                    except Exception:
                        pass
                    try:
                        psub = await js.pull_subscribe(subject, durable=durable)
                        err_streak = 0
                        log.info(f"{tag} resubscribed")
                    except Exception as e2:
                        log.error(f"{tag} resubscribe failed: {e2}")
                try:
                    await asyncio.wait_for(stop.wait(), timeout=1)
                    return
                except asyncio.TimeoutError:
                    continue

            if not msgs:
                sem.release()
                continue

            for msg in msgs:
                if is_rpc:
                    await run_rpc_msg(msg, handler_factory, tag, nc)
                else:
                    await run_with_lifecycle(msg, handler_factory, tag)
        except BaseException:
            try:
                sem.release()
            except ValueError:
                pass
            raise
        else:
            sem.release()


def _route_ai(models, subject: str, payload: dict):
    if subject == subj.AI_DETECT_LANGUAGE:
        return ai.detect_language(models, payload)
    if subject == subj.AI_SEARCH_QUERIES:
        return ai.search_queries(models, payload)
    if subject == subj.AI_RANK_LYRICS:
        return ai.rank_lyrics(models, payload)
    if subject == subj.AI_TRANSCRIBE:
        return transcribe(models, payload)
    if subject == subj.AI_ENCODE_TEXT_MULAN:
        return ai.encode_text_mulan(models, payload)
    if subject == subj.AI_RESOLVE_ARTIST:
        return resolve_artist(models, payload)
    if subject == subj.AI_VERIFY_EXISTENCE:
        return verify_existence(models, payload)
    if subject == subj.AI_MATCH_TRACK:
        return match_track(models, payload)
    if subject == subj.AI_QUALITY_SCORE:
        return quality_handler.score(models, payload)
    raise ValueError(f"unknown AI subject: {subject}")


async def main() -> None:
    nc = await connect()
    js = nc.jetstream()

    sems = _build_semaphores()
    enabled_tags = {tag for tag, sem in sems.items() if sem is not None}
    if not enabled_tags:
        log.error("All tags disabled in WORKER_CONCURRENCY — nothing to do, exiting.")
        return

    models = load_all(enabled_tags)
    qdrant = new_client()
    ensure_collections(qdrant)

    # Стримы создаёт backend. Воркер регистрирует только consumer'ы для
    # активных тэгов — отключённые стримы остаются другим воркерам.
    if "ai" in enabled_tags:
        await ensure_consumer(
            js, subj.STREAM_AI_RPC, subj.DURABLE_AI_RPC, subj.SUBJECT_AI_RPC_FILTER,
        )
    if "audio" in enabled_tags:
        await ensure_consumer(
            js, subj.STREAM_INDEX_AUDIO, subj.DURABLE_INDEX_AUDIO, subj.SUBJECT_INDEX_AUDIO_NEW
        )
    if "lyrics" in enabled_tags:
        await ensure_consumer(
            js, subj.STREAM_EMBED_LYRICS, subj.DURABLE_EMBED_LYRICS, subj.SUBJECT_EMBED_LYRICS_NEW
        )
    if "collab" in enabled_tags:
        await ensure_consumer(
            js, subj.STREAM_TRAIN_COLLAB, subj.DURABLE_TRAIN_COLLAB, subj.SUBJECT_TRAIN_COLLAB_NEW
        )
    if "quality" in enabled_tags:
        await ensure_consumer(
            js,
            subj.STREAM_TRAIN_QUALITY,
            subj.DURABLE_TRAIN_QUALITY,
            subj.SUBJECT_TRAIN_QUALITY_NEW,
        )

    stop = asyncio.Event()

    def _signal(*_):
        if stop.is_set():
            log.warning("second signal received, forcing exit")
            os._exit(0)
        log.info("signal received, stopping")
        stop.set()
        # Hard deadline: if we're still alive after 5s, something is blocked
        # in C-extension code (torch, demucs) that ignores asyncio cancel.
        threading.Timer(5.0, lambda: os._exit(0)).start()

    loop = asyncio.get_running_loop()
    for s in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(s, _signal)
        except NotImplementedError:
            pass

    tasks: list[asyncio.Task] = []
    if sems["ai"] is not None:
        tasks.append(asyncio.create_task(
            _js_pull_loop(
                js, sems["ai"], subj.STREAM_AI_RPC, subj.DURABLE_AI_RPC,
                subj.SUBJECT_AI_RPC_FILTER,
                lambda subject, payload: _route_ai(models, subject, payload),
                "[ai]", stop, is_rpc=True, nc=nc,
            )
        ))
    if sems["audio"] is not None:
        tasks.append(asyncio.create_task(
            _js_pull_loop(
                js, sems["audio"], subj.STREAM_INDEX_AUDIO, subj.DURABLE_INDEX_AUDIO,
                subj.SUBJECT_INDEX_AUDIO_NEW,
                lambda p: audio.handle(p, models, qdrant, nc),
                "[audio]", stop, is_rpc=False,
            )
        ))
    if sems["lyrics"] is not None:
        tasks.append(asyncio.create_task(
            _js_pull_loop(
                js, sems["lyrics"], subj.STREAM_EMBED_LYRICS, subj.DURABLE_EMBED_LYRICS,
                subj.SUBJECT_EMBED_LYRICS_NEW,
                lambda p: lyrics.handle(p, models, qdrant, nc),
                "[lyrics]", stop, is_rpc=False,
            )
        ))
    if sems["collab"] is not None:
        tasks.append(asyncio.create_task(
            _js_pull_loop(
                js, sems["collab"], subj.STREAM_TRAIN_COLLAB, subj.DURABLE_TRAIN_COLLAB,
                subj.SUBJECT_TRAIN_COLLAB_NEW,
                lambda p: collab_handler.handle(p, models, qdrant, nc),
                "[collab]", stop, is_rpc=False,
            )
        ))
    if sems["quality"] is not None:
        tasks.append(asyncio.create_task(
            _js_pull_loop(
                js, sems["quality"], subj.STREAM_TRAIN_QUALITY, subj.DURABLE_TRAIN_QUALITY,
                subj.SUBJECT_TRAIN_QUALITY_NEW,
                lambda p: quality_handler.handle(p, models, qdrant, nc),
                "[quality]", stop, is_rpc=False,
            )
        ))

    log.info(f"Worker ready ({len(tasks)} pull-loops active).")
    await stop.wait()

    for t in tasks:
        t.cancel()
    await asyncio.gather(*tasks, return_exceptions=True)
    try:
        await asyncio.wait_for(nc.drain(), timeout=2)
    except (asyncio.TimeoutError, Exception) as e:
        log.warning(f"nc.drain timeout/failed: {e}")


if __name__ == "__main__":
    asyncio.run(main())
