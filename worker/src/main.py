"""Воркер = AI-слой над NATS (JetStream). Параллелизм — WORKER_CONCURRENCY. См. AGENTS.md."""
import asyncio
import logging
import os
import signal
import threading

from . import subjects as subj
from .bus import (
    StreamUnavailable,
    connect,
    ensure_consumer,
    ensure_limits_stream,
    ensure_work_queue_stream,
)
from .config import (
    BATCH_WAIT_MS,
    LYRICS_BATCH,
    TRANSCRIBE_HARD_TIMEOUT_SEC,
    WORKER_CONCURRENCY,
)
from .handlers import ai, audio, lyrics
from .handlers import collab as collab_handler
from .handlers import encode as encode_handler
from .handlers import quality as quality_handler
from .handlers import transcribe as transcribe_handler
from .handlers.resolve import match_track, resolve_artist, verify_existence
from .models import load_all
from .runner import run_batched_lane, run_concurrent_lane

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
for noisy in ("httpx", "httpcore", "urllib3", "huggingface_hub", "filelock"):
    logging.getLogger(noisy).setLevel(logging.WARNING)
log = logging.getLogger(__name__)

TAGS = ("ai", "audio", "lyrics", "collab", "quality", "transcribe")


def _build_concurrency() -> tuple[dict[str, int], set[str]]:
    """{tag: N} + множество включённых тэгов (N>0). Глобальный int → N на каждый тэг."""
    cfg = WORKER_CONCURRENCY
    if isinstance(cfg, int):
        conc = {tag: cfg for tag in TAGS}
    else:
        conc = {tag: cfg.get(tag, 1) for tag in TAGS}
    enabled = {t for t, n in conc.items() if n > 0}
    log.info(
        "WORKER_CONCURRENCY: "
        + ", ".join(f"{t}={conc[t]}" + (" [OFF]" if conc[t] == 0 else "") for t in TAGS)
    )
    return conc, enabled


def _route_ai(models, subject: str, payload: dict):
    if subject == subj.AI_DETECT_LANGUAGE:
        return ai.detect_language(models, payload)
    if subject == subj.AI_SEARCH_QUERIES:
        return ai.search_queries(models, payload)
    if subject == subj.AI_RANK_LYRICS:
        return ai.rank_lyrics(models, payload)
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

    conc, enabled = _build_concurrency()
    if not enabled:
        log.error("All tags disabled in WORKER_CONCURRENCY — nothing to do, exiting.")
        return

    # Лейн = его work-queue стрим(ы) + consumer'ы. (stream, subjects, durable, filter).
    # ai тащит ещё и encode-лейн (делит модели mulan/bge-m3 → гейтится тем же тэгом).
    lane_specs: dict[str, list[tuple[str, list[str], str, str]]] = {
        "ai": [
            (subj.STREAM_AI_RPC, ["ai.rpc.>"], subj.DURABLE_AI_RPC, subj.SUBJECT_AI_RPC_FILTER),
            (subj.STREAM_ENCODE, ["encode.>"], subj.DURABLE_ENCODE, subj.SUBJECT_ENCODE_NEW),
        ],
        "audio": [
            (subj.STREAM_INDEX_AUDIO, ["index.audio.>"], subj.DURABLE_INDEX_AUDIO, subj.SUBJECT_INDEX_AUDIO_NEW),
        ],
        "lyrics": [
            (subj.STREAM_EMBED_LYRICS, ["embed.lyrics.>"], subj.DURABLE_EMBED_LYRICS, subj.SUBJECT_EMBED_LYRICS_NEW),
        ],
        "collab": [
            (subj.STREAM_TRAIN_COLLAB, ["train.collab.>"], subj.DURABLE_TRAIN_COLLAB, subj.SUBJECT_TRAIN_COLLAB_NEW),
        ],
        "quality": [
            (subj.STREAM_TRAIN_QUALITY, ["train.quality.>"], subj.DURABLE_TRAIN_QUALITY, subj.SUBJECT_TRAIN_QUALITY_NEW),
        ],
        "transcribe": [
            (subj.STREAM_TRANSCRIBE, ["transcribe.>"], subj.DURABLE_TRANSCRIBE, subj.SUBJECT_TRANSCRIBE_NEW),
        ],
    }

    # Стримы + consumer'ы. Лейн, чей стрим недоступен на этом NATS (публичная
    # нода вне бриджуемых брокером лейнов), ОТКЛЮЧАЕМ — а не роняем весь воркер.
    # Сетевые ошибки/недоступность NATS — наоборот, ретраим с backoff (стрим
    # просто ещё не готов: wipe volume, рестарт, сетевой glitch).
    async def _provision_streams() -> None:
        # done.* — общий стрим публикации результатов, нужен любому лейну.
        try:
            await ensure_limits_stream(js, "PIPELINE_DONE", ["done.>"])
        except StreamUnavailable:
            log.warning("PIPELINE_DONE отсутствует и не создаётся; done.* может не сохраняться")
        for tag in [t for t in TAGS if t in enabled]:
            try:
                for stream, subjects, durable, flt in lane_specs[tag]:
                    await ensure_work_queue_stream(js, stream, subjects)
                    await ensure_consumer(js, stream, durable, flt)
            except StreamUnavailable as e:
                log.warning(f"лейн '{tag}' отключён: стрим {e} недоступен на этом NATS")
                enabled.discard(tag)

    backoff = 2
    while True:
        try:
            await _provision_streams()
            break
        except Exception as e:
            log.warning(f"NATS stream/consumer setup failed ({e}), retrying in {backoff}s…")
            await asyncio.sleep(backoff)
            backoff = min(backoff * 2, 30)

    if not enabled:
        log.error("Ни один лейн не доступен на этом NATS — нечего делать, выходим.")
        return

    # Модели грузим ПОСЛЕ provisioning — чтобы не тратить VRAM на лейны, которые
    # этот NATS не обслуживает (публичная нода отключила ai/collab/quality выше).
    models = load_all(enabled)

    stop = asyncio.Event()

    def _signal(*_):
        if stop.is_set():
            log.warning("second signal received, forcing exit")
            os._exit(0)
        log.info("signal received, stopping")
        stop.set()
        # Hard deadline: если живы через 5с — что-то залипло в C-ext (torch/demucs).
        threading.Timer(5.0, lambda: os._exit(0)).start()

    loop = asyncio.get_running_loop()
    for s in (signal.SIGINT, signal.SIGTERM):
        try:
            loop.add_signal_handler(s, _signal)
        except NotImplementedError:
            pass

    # GPU-лок нужен, только когда ai шарит модель с батч-лейном (иначе один владелец).
    ai_on = "ai" in enabled
    audio_gpu_lock = models.mulan_lock if ai_on else None
    lyrics_gpu_lock = models.lyrics_text_lock if ai_on else None

    tasks: list[asyncio.Task] = []
    if "ai" in enabled:
        tasks.append(asyncio.create_task(
            run_concurrent_lane(
                js, asyncio.Semaphore(conc["ai"]), subj.STREAM_AI_RPC, subj.DURABLE_AI_RPC,
                subj.SUBJECT_AI_RPC_FILTER,
                lambda subject, payload: _route_ai(models, subject, payload),
                "[ai]", stop, is_rpc=True, nc=nc,
            )
        ))
        # Энкод запросов — отдельный work-queue лейн (НЕ rpc): считает вектор и
        # публикует done.encode. Делит GPU-локи mulan/bge-m3 с ai-лейном.
        tasks.append(asyncio.create_task(
            run_concurrent_lane(
                js, asyncio.Semaphore(conc["ai"]), subj.STREAM_ENCODE, subj.DURABLE_ENCODE,
                subj.SUBJECT_ENCODE_NEW,
                lambda p: encode_handler.handle(p, models, nc),
                "[encode]", stop, is_rpc=False, nc=nc,
            )
        ))
    if "audio" in enabled:
        tasks.append(asyncio.create_task(
            run_batched_lane(
                js, models, nc,
                stream=subj.STREAM_INDEX_AUDIO, durable=subj.DURABLE_INDEX_AUDIO,
                subject=subj.SUBJECT_INDEX_AUDIO_NEW, tag="[audio]", stop=stop,
                prepare=audio.prepare, gpu_batch=audio.gpu_batch, publish=audio.publish,
                fanout=conc["audio"], max_batch=1, wait_ms=0, gpu_lock=audio_gpu_lock,
            )
        ))
    if "lyrics" in enabled:
        tasks.append(asyncio.create_task(
            run_batched_lane(
                js, models, nc,
                stream=subj.STREAM_EMBED_LYRICS, durable=subj.DURABLE_EMBED_LYRICS,
                subject=subj.SUBJECT_EMBED_LYRICS_NEW, tag="[lyrics]", stop=stop,
                prepare=lyrics.prepare, gpu_batch=lyrics.gpu_batch, publish=lyrics.publish,
                publish_skip=lyrics.publish_skip,
                fanout=conc["lyrics"], max_batch=LYRICS_BATCH, wait_ms=BATCH_WAIT_MS,
                gpu_lock=lyrics_gpu_lock,
            )
        ))
    if "collab" in enabled:
        tasks.append(asyncio.create_task(
            run_concurrent_lane(
                js, asyncio.Semaphore(conc["collab"]), subj.STREAM_TRAIN_COLLAB,
                subj.DURABLE_TRAIN_COLLAB, subj.SUBJECT_TRAIN_COLLAB_NEW,
                lambda p: collab_handler.handle(p, models, nc),
                "[collab]", stop, is_rpc=False,
            )
        ))
    if "quality" in enabled:
        tasks.append(asyncio.create_task(
            run_concurrent_lane(
                js, asyncio.Semaphore(conc["quality"]), subj.STREAM_TRAIN_QUALITY,
                subj.DURABLE_TRAIN_QUALITY, subj.SUBJECT_TRAIN_QUALITY_NEW,
                lambda p: quality_handler.handle(p, models, nc),
                "[quality]", stop, is_rpc=False,
            )
        ))
    if "transcribe" in enabled:
        tasks.append(asyncio.create_task(
            run_concurrent_lane(
                js, asyncio.Semaphore(conc["transcribe"]), subj.STREAM_TRANSCRIBE,
                subj.DURABLE_TRANSCRIBE, subj.SUBJECT_TRANSCRIBE_NEW,
                lambda p: transcribe_handler.handle(p, models, nc),
                "[transcribe]", stop, is_rpc=False, hard_timeout=TRANSCRIBE_HARD_TIMEOUT_SEC,
            )
        ))

    log.info(f"Worker ready ({len(tasks)} lanes active).")
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
