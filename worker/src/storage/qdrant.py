"""Qdrant helpers. Воркер пишет вектора, больше ни с чем не взаимодействует."""
import logging
import time

from qdrant_client import QdrantClient
from qdrant_client.models import Distance, PointStruct, VectorParams

from ..config import QDRANT_API_KEY, QDRANT_URL

log = logging.getLogger(__name__)

COLLECTIONS = {
    # OpenMuQ/MuQ-large-msd-iter: 1024-dim (замена MERT).
    "tracks_mert": 1024,
    # OpenMuQ/MuQ-MuLan-large: 512-dim (замена laion CLAP).
    "tracks_clap": 512,
    "tracks_lyrics": 1024,
}


def new_client() -> QdrantClient:
    return QdrantClient(url=QDRANT_URL, api_key=QDRANT_API_KEY)


def ensure_collections(client: QdrantClient) -> None:
    existing = {c.name: c for c in client.get_collections().collections}
    for name, size in COLLECTIONS.items():
        if name in existing:
            info = client.get_collection(name)
            params = info.config.params.vectors
            actual = params.size if hasattr(params, "size") else None
            if actual != size:
                log.warning(
                    f"Qdrant collection {name} dim mismatch (got {actual}, want {size}) — recreating"
                )
                client.delete_collection(name)
            else:
                continue
        client.create_collection(
            name, vectors_config=VectorParams(size=size, distance=Distance.COSINE)
        )
        log.info(f"Qdrant collection created: {name} (size={size})")


def upsert_audio(
    client: QdrantClient,
    sc_track_id: str,
    mert_vec: list[float],
    clap_vec: list[float],
    language: str | None,
) -> None:
    payload = {"sc_track_id": sc_track_id, "indexed_at": int(time.time())}
    if language:
        payload["language"] = language
    point_id = int(sc_track_id)
    client.upsert(
        "tracks_mert", points=[PointStruct(id=point_id, vector=mert_vec, payload=payload)]
    )
    client.upsert(
        "tracks_clap", points=[PointStruct(id=point_id, vector=clap_vec, payload=payload)]
    )


def upsert_lyrics(
    client: QdrantClient,
    sc_track_id: str,
    vec: list[float],
    language: str | None,
) -> None:
    payload = {"sc_track_id": sc_track_id, "embedded_at": int(time.time())}
    if language:
        payload["language"] = language
    client.upsert(
        "tracks_lyrics",
        points=[PointStruct(id=int(sc_track_id), vector=vec, payload=payload)],
    )


def has_audio_vectors(client: QdrantClient, sc_track_id: str) -> bool:
    try:
        points = client.retrieve("tracks_mert", ids=[int(sc_track_id)], with_payload=False)
        return len(points) > 0
    except Exception:
        return False


def has_lyrics_vector(client: QdrantClient, sc_track_id: str) -> bool:
    try:
        points = client.retrieve("tracks_lyrics", ids=[int(sc_track_id)], with_payload=False)
        return len(points) > 0
    except Exception:
        return False
