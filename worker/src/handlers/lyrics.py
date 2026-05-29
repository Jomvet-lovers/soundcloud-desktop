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


def _embed(models: Models, text: str) -> list[float]:
    vec = models.lyrics_embed.encode(text, normalize_embeddings=True)
    return vec.tolist()


async def handle(
    payload: dict,
    models: Models,
    nc: NATSClient,
) -> None:
    sc_track_id = str(payload["sc_track_id"])
    text = (payload.get("text") or "").strip()
    language = payload.get("language")

    if not text or len(text) < 30:
        log.debug(f"[lyrics] {sc_track_id} empty/short text, skip")
        await nc.publish(
            subj.SUBJECT_DONE_EMBED_LYRICS,
            json.dumps({"sc_track_id": sc_track_id, "skipped": True}).encode(),
        )
        return

    log.info(f"[lyrics] {sc_track_id} embedding ({len(text)} chars)")
    t0 = time.monotonic()
    vec = await asyncio.to_thread(_embed, models, text[:4000])
    log.info(f"[lyrics] {sc_track_id} embedded in {time.monotonic() - t0:.2f}s")

    done_payload: dict = {"sc_track_id": sc_track_id, "vec": vec}
    if language:
        done_payload["language"] = language
    await nc.publish(
        subj.SUBJECT_DONE_EMBED_LYRICS,
        json.dumps(done_payload).encode(),
    )
