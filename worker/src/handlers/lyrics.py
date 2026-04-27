"""EMBED_LYRICS: bge-m3 encode text → qdrant tracks_lyrics."""
import asyncio
import json
import logging
import time

from nats.aio.client import Client as NATSClient
from qdrant_client import QdrantClient

from .. import subjects as subj
from ..models import Models
from ..storage import has_lyrics_vector, upsert_lyrics

log = logging.getLogger(__name__)


def _embed(models: Models, text: str) -> list[float]:
    vec = models.lyrics_embed.encode(text, normalize_embeddings=True)
    return vec.tolist()


async def handle(
    payload: dict,
    models: Models,
    qdrant: QdrantClient,
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

    if has_lyrics_vector(qdrant, sc_track_id):
        log.info(f"[lyrics] {sc_track_id} already embedded, skip")
    else:
        log.info(f"[lyrics] {sc_track_id} embedding ({len(text)} chars)")
        t0 = time.monotonic()
        vec = await asyncio.to_thread(_embed, models, text[:4000])
        upsert_lyrics(qdrant, sc_track_id, vec, language)
        log.info(f"[lyrics] {sc_track_id} embedded in {time.monotonic() - t0:.2f}s")

    await nc.publish(
        subj.SUBJECT_DONE_EMBED_LYRICS,
        json.dumps({"sc_track_id": sc_track_id}).encode(),
    )
