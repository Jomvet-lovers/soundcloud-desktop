"""AI.TRANSCRIBE: download audio → (опц. demucs vocals) → whisper → LRC/plain."""
import asyncio
import logging
import tempfile
import time
from pathlib import Path

import aiohttp
import torch

from ..models import DEVICE, Models, ensure_demucs

log = logging.getLogger(__name__)

DOWNLOAD_TIMEOUT_SEC = 60


def _format_lrc_timestamp(seconds: float) -> str:
    minutes = int(seconds // 60)
    secs = seconds - minutes * 60
    return f"[{minutes:02d}:{secs:05.2f}]"


def _extract_vocals(models: Models, src_path: Path) -> Path:
    """Demucs → vocals stem → temp WAV 16kHz mono (готов для Whisper)."""
    import torchaudio
    from demucs.apply import apply_model
    from demucs.audio import convert_audio

    model = models.demucs
    wav, sr = torchaudio.load(str(src_path))
    wav = convert_audio(wav, sr, model.samplerate, model.audio_channels)
    ref = wav.mean(0)
    wav = (wav - ref.mean()) / (ref.std() + 1e-8)
    model_dtype = next(model.parameters()).dtype
    with torch.no_grad():
        sources = apply_model(
            model,
            wav[None].to(DEVICE, dtype=model_dtype),
            device=DEVICE, split=True, overlap=0.25, progress=False,
        )[0]
    sources = sources.float() * ref.std() + ref.mean()
    vocals = sources[model.sources.index("vocals")].cpu()
    vocals_mono = vocals.mean(dim=0, keepdim=True)
    vocals_16k = torchaudio.transforms.Resample(model.samplerate, 16000)(vocals_mono)
    out = tempfile.NamedTemporaryFile(suffix=".vocals.wav", delete=False)
    out.close()
    torchaudio.save(out.name, vocals_16k, 16000)
    return Path(out.name)


async def transcribe(models: Models, payload: dict) -> dict:
    audio_url = payload.get("audio_url")
    language = payload.get("language")
    initial_prompt = payload.get("initial_prompt")
    isolate_vocals = payload.get("isolate_vocals", True)
    if not audio_url:
        raise ValueError("audio_url is empty")

    log.info(
        f"[transcribe] start url={audio_url} lang={language} "
        f"isolate_vocals={isolate_vocals} has_prompt={bool(initial_prompt)}"
    )

    tmp = tempfile.NamedTemporaryFile(suffix=".audio", delete=False)
    tmp_path = Path(tmp.name)
    vocals_path: Path | None = None
    try:
        t0 = time.monotonic()
        async with aiohttp.ClientSession() as session:
            async with session.get(
                audio_url, timeout=aiohttp.ClientTimeout(total=DOWNLOAD_TIMEOUT_SEC)
            ) as resp:
                resp.raise_for_status()
                async for chunk in resp.content.iter_chunked(64 * 1024):
                    tmp.write(chunk)
        tmp.close()
        size = tmp_path.stat().st_size
        log.info(f"[transcribe] downloaded {size} bytes in {time.monotonic() - t0:.2f}s")

        src_for_whisper = tmp_path
        if isolate_vocals:
            try:
                async with models.demucs_lock:
                    demucs_model = await asyncio.to_thread(ensure_demucs, models)
                    if demucs_model is not None:
                        t_demucs = time.monotonic()
                        vocals_path = await asyncio.to_thread(_extract_vocals, models, tmp_path)
                        src_for_whisper = vocals_path
                        log.info(
                            f"[transcribe] demucs vocals extracted in {time.monotonic() - t_demucs:.2f}s"
                        )
                    else:
                        log.info("[transcribe] demucs unavailable, using raw audio")
            except Exception as e:
                log.warning(f"[transcribe] vocal separation failed ({e}), raw audio fallback")

        def _run() -> dict:
            t_whisper = time.monotonic()
            segments, info = models.whisper.transcribe(
                str(src_for_whisper),
                language=language,
                vad_filter=True,
                word_timestamps=False,
                beam_size=5,
                initial_prompt=initial_prompt,
                condition_on_previous_text=False,
                no_speech_threshold=0.6,
                log_prob_threshold=-1.0,
            )
            lrc_lines: list[str] = []
            plain_lines: list[str] = []
            skipped_no_speech = 0
            skipped_low_prob = 0
            skipped_repeat = 0
            last_text: str | None = None
            repeat_count = 0
            for seg in segments:
                text = (seg.text or "").strip()
                if not text:
                    continue
                if getattr(seg, "no_speech_prob", 0.0) > 0.6:
                    skipped_no_speech += 1
                    continue
                if getattr(seg, "avg_logprob", 0.0) < -1.0:
                    skipped_low_prob += 1
                    continue
                if text == last_text:
                    repeat_count += 1
                    if repeat_count >= 2:
                        skipped_repeat += 1
                        continue
                else:
                    repeat_count = 0
                    last_text = text
                lrc_lines.append(f"{_format_lrc_timestamp(seg.start)}{text}")
                plain_lines.append(text)
            plain_chars = sum(len(x) for x in plain_lines)
            log.info(
                f"[transcribe] whisper done in {time.monotonic() - t_whisper:.2f}s "
                f"language={info.language} lines={len(plain_lines)} chars={plain_chars} "
                f"skipped(no_speech={skipped_no_speech}, low_prob={skipped_low_prob}, "
                f"repeat={skipped_repeat})"
            )
            if plain_chars < 30 or len(plain_lines) < 3:
                log.info(
                    f"[transcribe] result below noise threshold "
                    f"(chars={plain_chars}, lines={len(plain_lines)}), returning null"
                )
                return {"syncedLrc": None, "plainText": None, "language": info.language}
            return {
                "syncedLrc": "\n".join(lrc_lines),
                "plainText": "\n".join(plain_lines),
                "language": info.language,
            }

        async with models.whisper_lock:
            return await asyncio.to_thread(_run)
    finally:
        tmp_path.unlink(missing_ok=True)
        if vocals_path is not None:
            vocals_path.unlink(missing_ok=True)
        if DEVICE == "cuda":
            torch.cuda.empty_cache()
