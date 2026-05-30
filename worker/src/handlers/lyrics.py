"""EMBED_LYRICS: bge-m3 encode text → вектор в шину.

Запись в Qdrant — на бэке (см. AGENTS.md): воркер шлёт вектор в `done.embed_lyrics`.
"""
import asyncio
import json
import logging
import time
from nats.aio.client import Client as NATSClient

from .. import subjects as subj
from ..models import Models

log = logging.getLogger(__name__)


def _embed_batch(models: Models, texts: list[str]) -> list[list[float]]:
    """Один encode на пачку (SentenceTransformer сам маскирует паддинг)."""
    vecs = models.lyrics_embed.encode(
        texts, normalize_embeddings=True, batch_size=len(texts)
    )
    return [v.tolist() for v in vecs]


async def prepare(payload: dict, models: Models) -> dict | None:
    """None → пропуск пустого/короткого текста."""
    text = (payload.get("text") or "").strip()
    if not text or len(text) < 30:
        return None
    return {
        "sc_track_id": str(payload["sc_track_id"]),
        "text": text[:4000],
        "language": payload.get("language"),
    }


def gpu_batch(models: Models, items: list[dict]) -> list[dict]:
    t0 = time.monotonic()
    vecs = _embed_batch(models, [p["text"] for p in items])
    log.info(f"[lyrics] embedded batch×{len(items)} in {time.monotonic() - t0:.2f}s")
    return [{"vec": v, "language": p.get("language")} for p, v in zip(items, vecs)]


async def publish(nc: NATSClient, payload: dict, result: dict) -> None:
    done_payload: dict = {"sc_track_id": str(payload["sc_track_id"]), "vec": result["vec"]}
    if result.get("language"):
        done_payload["language"] = result["language"]
    await nc.publish(subj.SUBJECT_DONE_EMBED_LYRICS, json.dumps(done_payload).encode())


async def publish_skip(nc: NATSClient, payload: dict, _result) -> None:
    await nc.publish(
        subj.SUBJECT_DONE_EMBED_LYRICS,
        json.dumps({"sc_track_id": str(payload["sc_track_id"]), "skipped": True}).encode(),
    )
