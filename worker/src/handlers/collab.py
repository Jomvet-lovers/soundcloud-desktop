"""TRAIN_COLLAB: gensim Word2Vec на сессиях прослушивания → вектора в шину.

Модель обучается на последовательностях track_id внутри пользовательской сессии
(skip-gram, item2vec). Получившиеся вектора отражают «треки слушают вместе».

Это поведенческий сигнал — он работает там, где аудио-эмбеддинги (MuQ/CLAP/lyrics)
ломаются из-за высокой baseline-correlation. Используется как primary signal в
рекомендациях: retrieval + rerank.

Запись в Qdrant — на бэке (см. AGENTS.md): вектора едут блобом в Object Store
(в сообщение не лезут), в `done.train_collab` — имя объекта + dim.

Вход (NATS payload):
  {
    "object": "collab-<ts>",               # имя блоба сессий в Object Store
    "dim": 128,                            # размерность эмбеддинга
    "min_count": 3,                        # отсекать треки с <3 повторами
    "window": 5,
    "epochs": 5,
    "negative": 10
  }

Выход:
  put Object Store <object>-vectors {dim, points:[{id, vec}]}
  publish done.train_collab {trained, object, dim, vocab_size, n_sessions, train_sec}
"""
import asyncio
import json
import logging
import time
from nats.aio.client import Client as NATSClient

from .. import subjects as subj

log = logging.getLogger(__name__)


def _train(
    sessions: list[list[int]],
    dim: int,
    min_count: int,
    window: int,
    epochs: int,
    negative: int,
):
    # gensim импортируем лениво — чтобы отсутствие либы не крашило весь воркер
    # при старте через handlers/__init__.py.
    from gensim.models import Word2Vec

    str_sessions = [[str(t) for t in s] for s in sessions if len(s) >= 2]
    return Word2Vec(
        sentences=str_sessions,
        vector_size=dim,
        window=window,
        min_count=min_count,
        sg=1,                # skip-gram (item2vec стандарт)
        negative=negative,
        ns_exponent=0.75,
        epochs=epochs,
        workers=4,
        seed=42,
    )


async def handle(
    payload: dict,
    models,
    nc: NATSClient,
) -> None:
    object_name = payload.get("object")
    store = await nc.jetstream().object_store(subj.OBJECT_STORE_COLLAB)
    if object_name:
        obj = await store.get(object_name)
        sessions = json.loads(obj.data.decode())
        try:
            await store.delete(object_name)
        except Exception as e:
            log.debug(f"[collab] object delete failed: {e}")
    else:
        sessions = payload.get("sessions") or []
    dim = int(payload.get("dim") or 128)
    min_count = int(payload.get("min_count") or 3)
    window = int(payload.get("window") or 5)
    epochs = int(payload.get("epochs") or 5)
    negative = int(payload.get("negative") or 10)

    n_sessions = len(sessions)
    if n_sessions < 50:
        log.warning(f"[collab] too few sessions ({n_sessions}), skip")
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_COLLAB,
            json.dumps({"trained": False, "reason": "too_few_sessions", "n_sessions": n_sessions}).encode(),
        )
        return

    log.info(
        f"[collab] training: sessions={n_sessions} dim={dim} min_count={min_count} "
        f"window={window} epochs={epochs} negative={negative}"
    )
    t0 = time.monotonic()
    try:
        model = await asyncio.to_thread(
            _train, sessions, dim, min_count, window, epochs, negative
        )
    except ImportError as e:
        log.error(f"[collab] gensim not installed in worker image: {e}")
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_COLLAB,
            json.dumps({"trained": False, "reason": "gensim_missing", "error": str(e)}).encode(),
        )
        return
    train_sec = time.monotonic() - t0
    vocab = len(model.wv)
    log.info(f"[collab] trained in {train_sec:.2f}s vocab={vocab}")

    if vocab == 0:
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_COLLAB,
            json.dumps({"trained": False, "reason": "empty_vocab"}).encode(),
        )
        return

    points = [
        {"id": tid, "vec": model.wv[word].tolist()}
        for word in model.wv.index_to_key
        if (tid := _as_int(word)) is not None
    ]

    out_object = f"{object_name or 'collab'}-vectors-{int(time.time() * 1000)}"
    blob = json.dumps({"dim": dim, "points": points}).encode()
    await store.put(out_object, blob)
    upsert_sec = time.monotonic() - t0 - train_sec
    log.info(f"[collab] {len(points)} vectors → object {out_object} in {upsert_sec:.2f}s")

    await nc.publish(
        subj.SUBJECT_DONE_TRAIN_COLLAB,
        json.dumps(
            {
                "trained": True,
                "object": out_object,
                "vocab_size": len(points),
                "dim": dim,
                "n_sessions": n_sessions,
                "train_sec": round(train_sec, 2),
            }
        ).encode(),
    )


def _as_int(word: str) -> int | None:
    try:
        return int(word)
    except ValueError:
        return None
