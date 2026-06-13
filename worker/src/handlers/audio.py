"""INDEX_AUDIO: скачать трек с S3, посчитать MuQ + MuQ-MuLan, отдать вектора в шину.

Запись в Qdrant — на бэке (см. AGENTS.md): воркер шлёт вектора в `done.index_audio`.
"""
import aiohttp
import asyncio
import json
import logging
import tempfile
import time
import torch
import torchaudio
import torchaudio.functional as TAF
from nats.aio.client import Client as NATSClient

from . import _chromaprint
from .. import subjects as subj
from ..config import MAX_EMBED_DURATION_SEC
from ..models import DEVICE, Models

log = logging.getLogger(__name__)

DOWNLOAD_TIMEOUT_SEC = 90
_FP_MAX_SECONDS = 120  # как fpcalc -length 120: первые 120с трека в отпечаток


async def _download(url: str) -> bytes:
    async with aiohttp.ClientSession() as session:
        async with session.get(
            url, timeout=aiohttp.ClientTimeout(total=DOWNLOAD_TIMEOUT_SEC)
        ) as resp:
            resp.raise_for_status()
            return await resp.read()


def _fingerprint(waveform: torch.Tensor, sr: int) -> str | None:
    """chromaprint raw-fp из уже декоднутого PCM: первые 120с, нативные каналы,
    int16 interleaved. Совместимо с прежним `fpcalc -raw` по первым 64 символам."""
    clip = waveform[:, : _FP_MAX_SECONDS * sr]
    inter = clip.transpose(0, 1).contiguous()  # [samples, channels] interleaved
    pcm = (inter * 32768.0).round().clamp(-32768, 32767).to(torch.int16)
    try:
        return _chromaprint.raw_fingerprint(pcm.numpy().tobytes(), sr, clip.shape[0])
    except Exception as e:
        log.warning(f"[audio] fingerprint failed: {e}")
        return None


def _decode(audio_bytes: bytes) -> tuple[torch.Tensor, str | None]:
    """Один декод трека → (моно-волна 24кГц для MuQ, chromaprint-отпечаток).

    Один torchaudio-декод (ffmpeg-backend, читает AAC/MP3 нативно) кормит и MuQ,
    и отпечаток — раньше fpcalc декодил трек вторым проходом.

    Truncate MuQ-волны до MAX_EMBED_DURATION_SEC: MuQ attention O(T²) на длинном
    треке жрёт многие GB transient VRAM.
    """
    with tempfile.NamedTemporaryFile(suffix=".audio", delete=True) as f:
        f.write(audio_bytes)
        f.flush()
        waveform, orig_sr = torchaudio.load(f.name)  # [channels, samples] float32

    fp = _fingerprint(waveform, orig_sr)

    mono = waveform.mean(dim=0, keepdim=True) if waveform.shape[0] > 1 else waveform
    if orig_sr != 24000:
        mono = TAF.resample(mono, orig_sr, 24000)
    if MAX_EMBED_DURATION_SEC > 0:
        max_samples = MAX_EMBED_DURATION_SEC * 24000
        if mono.shape[1] > max_samples:
            mono = mono[:, :max_samples]
    return mono, fp  # [1, samples], fp|None


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


def _embed_mulan(models: Models, wav: torch.Tensor) -> list[float]:
    dtype = next(models.mulan.parameters()).dtype
    wavs = wav.to(DEVICE, dtype=dtype)
    with torch.no_grad():
        vec = models.mulan(wavs=wavs).squeeze()
    vec = vec / vec.norm()
    return vec.detach().float().cpu().numpy().tolist()


async def prepare(payload: dict, models: Models) -> dict | None:
    """Скачка + один декод (волна для MuQ + отпечаток) — всё вне GPU."""
    sc_track_id = str(payload["sc_track_id"])
    s3_url = payload["s3_url"]
    t0 = time.monotonic()
    try:
        audio_bytes = await _download(s3_url)
    except aiohttp.ClientResponseError as e:
        if e.status == 404:
            log.warning(f"[audio] {sc_track_id} not in S3 (404) — skipping")
            return None
        raise
    log.info(
        f"[audio] {sc_track_id} downloaded {len(audio_bytes)} bytes in "
        f"{time.monotonic() - t0:.2f}s"
    )
    wav, fingerprint = await asyncio.to_thread(_decode, audio_bytes)
    return {
        "sc_track_id": sc_track_id,
        "wav": wav,
        "fingerprint": fingerprint,
        "language": payload.get("language"),
    }


_OOM_RETRY_FRACTIONS = (1.0, 0.5, 0.25)


def _free_vram_mb() -> int:
    if DEVICE != "cuda":
        return -1
    return torch.cuda.mem_get_info()[0] // (1024 * 1024)


def _is_oom(e: Exception) -> bool:
    return isinstance(e, torch.cuda.OutOfMemoryError) or "out of memory" in str(e).lower()


def _embed_pair(models: Models, wav: torch.Tensor) -> tuple[list[float], list[float]]:
    """MuQ + MuLan с одного клипа, с откатом по OOM.

    Карта делится с десктопом: длинный трек ловит O(T²)-пик MuQ → OOM. Вместо
    nak'а (трек улетал бы в ретрай ×5) чистим кэш и ужимаем клип (¼ длины ≈ ×16
    меньше attention) до первого успеха. Оба вектора — с одного клипа, иначе
    mert/clap разойдутся по длине трека.
    """
    for i, frac in enumerate(_OOM_RETRY_FRACTIONS):
        clip = wav if frac == 1.0 else wav[:, : max(24000, int(wav.shape[1] * frac))]
        try:
            return _embed_muq(models, clip), _embed_mulan(models, clip)
        except (torch.cuda.OutOfMemoryError, RuntimeError) as e:
            if not _is_oom(e):
                raise
            torch.cuda.empty_cache()
            if i == len(_OOM_RETRY_FRACTIONS) - 1:
                raise
            log.warning(
                f"[audio] OOM на {clip.shape[1] / 24000:.0f}s клипе "
                f"(free {_free_vram_mb()} MiB) — empty_cache + укорот"
            )
            time.sleep(0.3)
    raise RuntimeError("unreachable")


def gpu_batch(models: Models, items: list[dict]) -> list[dict]:
    """По одному треку на forward (MuQ/MuLan не маскируют паддинг → не склеиваем)."""
    out: list[dict] = []
    try:
        for p in items:
            sc_track_id = p["sc_track_id"]
            t0 = time.monotonic()
            muq_vec, mulan_vec = _embed_pair(models, p["wav"])
            log.info(f"[audio] {sc_track_id} embedded in {time.monotonic() - t0:.2f}s")
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
