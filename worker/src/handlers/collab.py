"""TRAIN_COLLAB: gensim Word2Vec на сессиях прослушивания → qdrant tracks_collab.

Модель обучается на последовательностях track_id внутри пользовательской сессии
(skip-gram, item2vec). Получившиеся вектора отражают «треки слушают вместе».

Это поведенческий сигнал — он работает там, где аудио-эмбеддинги (MuQ/CLAP/lyrics)
ломаются из-за высокой baseline-correlation. Используется как primary signal в
рекомендациях: retrieval + rerank.

Вход (NATS payload):
  {
    "sessions": [[id1, id2, ...], ...],   # int track id; min len(session)=2
    "dim": 128,                            # размерность эмбеддинга
    "min_count": 3,                        # отсекать треки с <3 повторами
    "window": 5,
    "epochs": 5,
    "negative": 10
  }

Выход:
  publish done.train_collab {trained, vocab_size, dim, took_sec}
"""
import asyncio
import json
import logging
import time

from nats.aio.client import Client as NATSClient
from qdrant_client import QdrantClient
from qdrant_client.models import Distance, PointStruct, VectorParams

from .. import subjects as subj

log = logging.getLogger(__name__)

COLLAB_COLLECTION = "tracks_collab"


def _ensure_collection(client: QdrantClient, dim: int) -> None:
    existing = {c.name for c in client.get_collections().collections}
    if COLLAB_COLLECTION in existing:
        info = client.get_collection(COLLAB_COLLECTION)
        params = info.config.params.vectors
        actual = params.size if hasattr(params, "size") else None
        if actual == dim:
            return
        log.warning(
            f"[collab] collection {COLLAB_COLLECTION} dim mismatch (got {actual}, want {dim}) — recreating"
        )
        client.delete_collection(COLLAB_COLLECTION)
    client.create_collection(
        COLLAB_COLLECTION,
        vectors_config=VectorParams(size=dim, distance=Distance.COSINE),
    )
    log.info(f"[collab] collection {COLLAB_COLLECTION} created (dim={dim})")


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
    qdrant: QdrantClient,
    nc: NATSClient,
) -> None:
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

    _ensure_collection(qdrant, dim)

    # Батчевый upsert.
    BATCH = 500
    points: list[PointStruct] = []
    total = 0
    for word in model.wv.index_to_key:
        try:
            tid = int(word)
        except ValueError:
            continue
        vec = model.wv[word].tolist()
        points.append(PointStruct(id=tid, vector=vec, payload={"sc_track_id": str(tid)}))
        if len(points) >= BATCH:
            qdrant.upsert(COLLAB_COLLECTION, points=points)
            total += len(points)
            points = []
    if points:
        qdrant.upsert(COLLAB_COLLECTION, points=points)
        total += len(points)

    upsert_sec = time.monotonic() - t0 - train_sec
    log.info(f"[collab] upserted {total} vectors in {upsert_sec:.2f}s")

    await nc.publish(
        subj.SUBJECT_DONE_TRAIN_COLLAB,
        json.dumps(
            {
                "trained": True,
                "vocab_size": total,
                "dim": dim,
                "n_sessions": n_sessions,
                "train_sec": round(train_sec, 2),
                "upsert_sec": round(upsert_sec, 2),
            }
        ).encode(),
    )
