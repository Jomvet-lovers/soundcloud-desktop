"""INDEX_AUDIO: скачать трек с S3, посчитать MuQ + MuQ-MuLan, отдать вектора в шину.

Запись в Qdrant — на бэке (см. AGENTS.md): воркер шлёт вектора в `done.index_audio`.
"""
import aiohttp
import asyncio
import json
import logging
import shutil
import subprocess
import tempfile
import time
import torch
import torchaudio
import torchaudio.functional as TAF
from nats.aio.client import Client as NATSClient

from .. import subjects as subj
from ..config import MAX_EMBED_DURATION_SEC
from ..models import DEVICE, Models

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
    """Декод произвольного аудио (m4a/mp3/wav/...) в моно-волну заданного sr.

    Через torchaudio (ffmpeg-backend) — читает AAC/MP3 нативно, без librosa
    + audioread (deprecated в librosa 0.10, удалят в 1.0).

    Truncate до MAX_EMBED_DURATION_SEC — иначе MuQ attention (O(T²)) на
    длинных треках жрёт многие GB transient VRAM (для 10-min трека — ~9 GB
    одной матрицей attention, OOM-ит даже на 24 GB карте при параллельных
    задачах).
    """
    with tempfile.NamedTemporaryFile(suffix=".audio", delete=True) as f:
        f.write(audio_bytes)
        f.flush()
        waveform, orig_sr = torchaudio.load(f.name)  # [channels, samples]
    if waveform.shape[0] > 1:
        waveform = waveform.mean(dim=0, keepdim=True)
    if orig_sr != sr:
        waveform = TAF.resample(waveform, orig_sr, sr)
    if MAX_EMBED_DURATION_SEC > 0:
        max_samples = MAX_EMBED_DURATION_SEC * sr
        if waveform.shape[1] > max_samples:
            waveform = waveform[:, :max_samples]
    return waveform  # [1, samples]


def _embed_muq(models: Models, wav: torch.Tensor) -> list[float]:
    dtype = next(models.muq.parameters()).dtype
    wavs = wav.to(DEVICE, dtype=dtype)
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


def _chromaprint_fingerprint(audio_bytes: bytes) -> str | None:
    """Возвращает chromaprint (compressed string) для audio_bytes или None.
    Использует системный fpcalc; если бинаря нет — тихо отдаёт None."""
    fpcalc = shutil.which("fpcalc")
    if not fpcalc:
        return None
    with tempfile.NamedTemporaryFile(suffix=".audio", delete=True) as f:
        f.write(audio_bytes)
        f.flush()
        try:
            res = subprocess.run(
                [fpcalc, "-raw", "-length", "120", f.name],
                capture_output=True,
                timeout=30,
                check=False,
            )
        except (OSError, subprocess.TimeoutExpired) as e:
            log.warning(f"[fpcalc] failed: {e}")
            return None
    if res.returncode != 0:
        log.warning(f"[fpcalc] non-zero {res.returncode}: {res.stderr.decode(errors='ignore')[:200]}")
        return None
    for line in res.stdout.decode(errors="ignore").splitlines():
        if line.startswith("FINGERPRINT="):
            return line[len("FINGERPRINT="):].strip() or None
    return None


def _embed_mulan(models: Models, wav: torch.Tensor) -> list[float]:
    dtype = next(models.mulan.parameters()).dtype
    wavs = wav.to(DEVICE, dtype=dtype)
    with torch.no_grad():
        vec = models.mulan(wavs=wavs).squeeze()
    vec = vec / vec.norm()
    return vec.detach().float().cpu().numpy().tolist()


async def prepare(payload: dict, models: Models) -> dict:
    """Скачка + декод + chromaprint (всё вне GPU)."""
    sc_track_id = str(payload["sc_track_id"])
    s3_url = payload["s3_url"]
    t0 = time.monotonic()
    audio_bytes = await _download(s3_url)
    log.info(
        f"[audio] {sc_track_id} downloaded {len(audio_bytes)} bytes in "
        f"{time.monotonic() - t0:.2f}s"
    )
    wav = await asyncio.to_thread(_load_wav, audio_bytes, 24000)
    try:
        fingerprint = await asyncio.to_thread(_chromaprint_fingerprint, audio_bytes)
    except Exception as e:
        log.warning(f"[audio] {sc_track_id} chromaprint crashed: {e}")
        fingerprint = None
    return {
        "sc_track_id": sc_track_id,
        "wav": wav,
        "fingerprint": fingerprint,
        "language": payload.get("language"),
    }


def gpu_batch(models: Models, items: list[dict]) -> list[dict]:
    """По одному треку на forward (MuQ/MuLan не маскируют паддинг → не склеиваем)."""
    out: list[dict] = []
    try:
        for p in items:
            sc_track_id = p["sc_track_id"]
            wav = p["wav"]
            t_muq = time.monotonic()
            muq_vec = _embed_muq(models, wav)
            t_mulan = time.monotonic()
            mulan_vec = _embed_mulan(models, wav)
            log.info(
                f"[audio] {sc_track_id} muq {t_mulan - t_muq:.2f}s "
                f"mulan {time.monotonic() - t_mulan:.2f}s"
            )
            out.append(
                {
                    "mert": muq_vec,
                    "clap": mulan_vec,
                    "fingerprint": p.get("fingerprint"),
                    "language": p.get("language"),
                }
            )
    finally:
        if DEVICE == "cuda":
            torch.cuda.empty_cache()
    return out


async def publish(nc: NATSClient, payload: dict, result: dict) -> None:
    done_payload: dict = {
        "sc_track_id": str(payload["sc_track_id"]),
        "mert": result["mert"],
        "clap": result["clap"],
    }
    if result.get("language"):
        done_payload["language"] = result["language"]
    if result.get("fingerprint"):
        done_payload["fingerprint"] = result["fingerprint"]
    await nc.publish(subj.SUBJECT_DONE_INDEX_AUDIO, json.dumps(done_payload).encode())
