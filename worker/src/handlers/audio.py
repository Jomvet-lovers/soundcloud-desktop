"""INDEX_AUDIO: скачать трек с S3, посчитать MuQ + MuQ-MuLan, записать в qdrant."""
import asyncio
import io
import json
import logging
import tempfile
import time

import aiohttp
import librosa
import torch
from nats.aio.client import Client as NATSClient
from qdrant_client import QdrantClient

from .. import subjects as subj
from ..models import DEVICE, Models
from ..storage import has_audio_vectors, upsert_audio

log = logging.getLogger(__name__)

DOWNLOAD_TIMEOUT_SEC = 90


async def _download(url: str) -> bytes:
    async with aiohttp.ClientSession() as session:
        async with session.get(
            url, timeout=aiohttp.ClientTimeout(total=DOWNLOAD_TIMEOUT_SEC)
        ) as resp:
            resp.raise_for_status()
            return await resp.read()


def _load_wav(audio_bytes: bytes, sr: int) -> torch.Tensor:
    with tempfile.NamedTemporaryFile(suffix=".audio", delete=True) as f:
        f.write(audio_bytes)
        f.flush()
        audio, _ = librosa.load(f.name, sr=sr, mono=True)
    return torch.tensor(audio).unsqueeze(0)


def _embed_muq(models: Models, audio_bytes: bytes) -> list[float]:
    dtype = next(models.muq.parameters()).dtype
    wavs = _load_wav(audio_bytes, sr=24000).to(DEVICE, dtype=dtype)
    with torch.no_grad():
        out = models.muq(wavs, output_hidden_states=True)
    # Среднее по слоям через аккумулятор [1024] — не torch.stack, иначе пик памяти × num_layers.
    hidden = out.hidden_states
    acc = torch.zeros(1024, device=DEVICE, dtype=hidden[0].dtype)
    for h in hidden:
        acc += h.squeeze(0).mean(dim=0)
    acc = acc / len(hidden)
    acc = acc / acc.norm()
    return acc.detach().float().cpu().numpy().tolist()


def _embed_mulan(models: Models, audio_bytes: bytes) -> list[float]:
    dtype = next(models.mulan.parameters()).dtype
    wavs = _load_wav(audio_bytes, sr=24000).to(DEVICE, dtype=dtype)
    with torch.no_grad():
        vec = models.mulan(wavs=wavs).squeeze()
    vec = vec / vec.norm()
    return vec.detach().float().cpu().numpy().tolist()


async def handle(
    payload: dict,
    models: Models,
    qdrant: QdrantClient,
    nc: NATSClient,
) -> None:
    sc_track_id = str(payload["sc_track_id"])
    s3_url = payload["s3_url"]
    language = payload.get("language")

    # Идемпотентность: вектора уже есть → просто публикуем done.
    if has_audio_vectors(qdrant, sc_track_id):
        log.info(f"[audio] {sc_track_id} already indexed, skip embed")
    else:
        log.info(f"[audio] {sc_track_id} downloading {s3_url}")
        t0 = time.monotonic()
        audio_bytes = await _download(s3_url)
        log.info(
            f"[audio] {sc_track_id} downloaded {len(audio_bytes)} bytes in "
            f"{time.monotonic() - t0:.2f}s"
        )
        try:
            t_muq = time.monotonic()
            muq_vec = await asyncio.to_thread(_embed_muq, models, audio_bytes)
            log.info(f"[audio] {sc_track_id} muq done in {time.monotonic() - t_muq:.2f}s")
            async with models.mulan_lock:
                t_mulan = time.monotonic()
                mulan_vec = await asyncio.to_thread(_embed_mulan, models, audio_bytes)
                log.info(
                    f"[audio] {sc_track_id} mulan done in {time.monotonic() - t_mulan:.2f}s"
                )
            upsert_audio(qdrant, sc_track_id, muq_vec, mulan_vec, language)
            log.info(f"[audio] {sc_track_id} embedded + upserted")
        finally:
            if DEVICE == "cuda":
                torch.cuda.empty_cache()

    await nc.publish(
        subj.SUBJECT_DONE_INDEX_AUDIO,
        json.dumps({"sc_track_id": sc_track_id}).encode(),
    )
