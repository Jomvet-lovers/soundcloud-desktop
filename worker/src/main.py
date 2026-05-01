"""Воркер = только AI-слой. Shina — NATS (JetStream). HTTP/Redis не используются.

- Все задачи идут через JetStream pull-consumer'ы.
- Глобальный семафор `inference_sem(1)` гарантирует: один воркер = один инференс за раз.
  Пока семафор занят, воркер НЕ делает fetch — сообщения остаются в стриме и подхватываются
  другими воркерами (queue group через общий durable).
- Подтверждение "я работаю" раз в TASK_HEARTBEAT_SEC, жёсткий таймаут TASK_HARD_TIMEOUT_SEC.
"""
import asyncio
import logging
import os
import signal
import threading

from . import subjects as subj
from .bus import connect, ensure_consumer, run_rpc_msg, run_with_lifecycle
from .handlers import ai, audio, lyrics
from .handlers import collab as collab_handler
from .handlers import ltr as ltr_handler
from .handlers.transcribe import transcribe
from .models import load_all
from .storage import ensure_collections, new_client

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
for noisy in ("httpx", "httpcore", "urllib3", "huggingface_hub", "filelock"):
    logging.getLogger(noisy).setLevel(logging.WARNING)
log = logging.getLogger(__name__)


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
    """Пока inference_sem занят — не вызываем fetch, сообщения достаются другим воркерам.

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
    if subject == subj.AI_LTR_SCORE:
        return ltr_handler.score(models, payload)
    raise ValueError(f"unknown AI subject: {subject}")


async def main() -> None:
    nc = await connect()
    js = nc.jetstream()

    models = load_all()
    qdrant = new_client()
    ensure_collections(qdrant)

    # Стримы создаёт backend. Воркер только добавляет свои consumer'ы.
    await ensure_consumer(
        js, subj.STREAM_AI_RPC, subj.DURABLE_AI_RPC, subj.SUBJECT_AI_RPC_FILTER,
    )
    await ensure_consumer(
        js, subj.STREAM_INDEX_AUDIO, subj.DURABLE_INDEX_AUDIO, subj.SUBJECT_INDEX_AUDIO_NEW
    )
    await ensure_consumer(
        js, subj.STREAM_EMBED_LYRICS, subj.DURABLE_EMBED_LYRICS, subj.SUBJECT_EMBED_LYRICS_NEW
    )
    await ensure_consumer(
        js, subj.STREAM_TRAIN_COLLAB, subj.DURABLE_TRAIN_COLLAB, subj.SUBJECT_TRAIN_COLLAB_NEW
    )
    await ensure_consumer(
        js, subj.STREAM_TRAIN_LTR, subj.DURABLE_TRAIN_LTR, subj.SUBJECT_TRAIN_LTR_NEW
    )

    stop = asyncio.Event()
    inference_sem = asyncio.Semaphore(1)

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

    ai_task = asyncio.create_task(
        _js_pull_loop(
            js, inference_sem, subj.STREAM_AI_RPC, subj.DURABLE_AI_RPC,
            subj.SUBJECT_AI_RPC_FILTER,
            lambda subject, payload: _route_ai(models, subject, payload),
            "[ai]", stop, is_rpc=True, nc=nc,
        )
    )
    audio_task = asyncio.create_task(
        _js_pull_loop(
            js, inference_sem, subj.STREAM_INDEX_AUDIO, subj.DURABLE_INDEX_AUDIO,
            subj.SUBJECT_INDEX_AUDIO_NEW,
            lambda p: audio.handle(p, models, qdrant, nc),
            "[audio]", stop, is_rpc=False,
        )
    )
    lyrics_task = asyncio.create_task(
        _js_pull_loop(
            js, inference_sem, subj.STREAM_EMBED_LYRICS, subj.DURABLE_EMBED_LYRICS,
            subj.SUBJECT_EMBED_LYRICS_NEW,
            lambda p: lyrics.handle(p, models, qdrant, nc),
            "[lyrics]", stop, is_rpc=False,
        )
    )
    collab_task = asyncio.create_task(
        _js_pull_loop(
            js, inference_sem, subj.STREAM_TRAIN_COLLAB, subj.DURABLE_TRAIN_COLLAB,
            subj.SUBJECT_TRAIN_COLLAB_NEW,
            lambda p: collab_handler.handle(p, models, qdrant, nc),
            "[collab]", stop, is_rpc=False,
        )
    )
    ltr_task = asyncio.create_task(
        _js_pull_loop(
            js, inference_sem, subj.STREAM_TRAIN_LTR, subj.DURABLE_TRAIN_LTR,
            subj.SUBJECT_TRAIN_LTR_NEW,
            lambda p: ltr_handler.handle(p, models, qdrant, nc),
            "[ltr]", stop, is_rpc=False,
        )
    )

    log.info("Worker ready.")
    await stop.wait()

    ai_task.cancel()
    audio_task.cancel()
    lyrics_task.cancel()
    collab_task.cancel()
    ltr_task.cancel()
    await asyncio.gather(ai_task, audio_task, lyrics_task, collab_task, ltr_task, return_exceptions=True)
    try:
        await asyncio.wait_for(nc.drain(), timeout=2)
    except (asyncio.TimeoutError, Exception) as e:
        log.warning(f"nc.drain timeout/failed: {e}")


if __name__ == "__main__":
    asyncio.run(main())
