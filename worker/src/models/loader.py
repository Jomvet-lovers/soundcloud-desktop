"""Загрузка моделей один раз на старте воркера. Mini LLM и Demucs — лениво."""
import logging

import torch
from faster_whisper import WhisperModel
from muq import MuQ, MuQMuLan
from sentence_transformers import SentenceTransformer
from transformers import AutoModelForSequenceClassification, AutoTokenizer

from ..config import WHISPER_COMPUTE, WHISPER_MODEL
from .device import DEVICE, USE_FP16
from .registry import Models

log = logging.getLogger(__name__)


def _prepare(model, *, already_on_device: bool = False):
    model.requires_grad_(False)
    set_inference = getattr(model, "eval", None)
    if callable(set_inference):
        model = set_inference()
    if not already_on_device and hasattr(model, "to"):
        model = model.to(DEVICE)
    if USE_FP16 and hasattr(model, "half") and not already_on_device:
        model = model.half()
    return model


def load_all(enabled_tags: set[str] | None = None) -> Models:
    """Грузит только те модели, что нужны для активных тэгов.

    Карта зависимостей:
      muq, mulan   ← audio, ai (match_track, encode_text_mulan)
      bge-m3       ← lyrics, ai (rank_lyrics), collab, quality
      xlm-roberta  ← ai (detect_language)
      whisper      ← transcribe (self-gen лирика)
      Qwen, demucs — ленивые, грузятся при первом обращении (demucs — в transcribe).

    enabled_tags=None → грузим всё (старое поведение).
    """
    if enabled_tags is None:
        enabled_tags = {"ai", "audio", "lyrics", "collab", "quality", "transcribe"}

    need_muq = bool(enabled_tags & {"audio", "ai"})
    need_mulan = bool(enabled_tags & {"audio", "ai"})
    need_lyrics_embed = bool(enabled_tags & {"lyrics", "ai", "collab", "quality"})
    need_lang = "ai" in enabled_tags
    need_whisper = "transcribe" in enabled_tags

    log.info(f"Worker device: {DEVICE} (fp16={USE_FP16})")
    log.info(f"Loading models for tags: {sorted(enabled_tags)}")

    muq = None
    if need_muq:
        log.info("Loading MuQ (OpenMuQ/MuQ-large-msd-iter)...")
        muq = _prepare(MuQ.from_pretrained("OpenMuQ/MuQ-large-msd-iter"))

    mulan = None
    if need_mulan:
        log.info("Loading MuQ-MuLan (OpenMuQ/MuQ-MuLan-large)...")
        mulan = _prepare(MuQMuLan.from_pretrained("OpenMuQ/MuQ-MuLan-large"))

    lyrics_embed = None
    if need_lyrics_embed:
        log.info("Loading bge-m3...")
        st_extra = {"model_kwargs": {"torch_dtype": torch.float16}} if USE_FP16 else {}
        lyrics_embed = SentenceTransformer("BAAI/bge-m3", device=DEVICE, **st_extra)

    lang_tokenizer = None
    lang_model = None
    lang_id2label: dict = {}
    if need_lang:
        log.info("Loading xlm-roberta language detector...")
        lang_name = "papluca/xlm-roberta-base-language-detection"
        lang_tokenizer = AutoTokenizer.from_pretrained(lang_name)
        lang_model = _prepare(AutoModelForSequenceClassification.from_pretrained(lang_name))
        lang_id2label = lang_model.config.id2label

    whisper = None
    if need_whisper:
        log.info(f"Loading Whisper ({WHISPER_MODEL})...")
        compute_type = WHISPER_COMPUTE or ("float16" if USE_FP16 else "int8")
        whisper = WhisperModel(WHISPER_MODEL, device=DEVICE, compute_type=compute_type)

    log.info("All models loaded.")
    return Models(
        muq=muq,
        mulan=mulan,
        lyrics_embed=lyrics_embed,
        lang_tokenizer=lang_tokenizer,
        lang_model=lang_model,
        lang_id2label=lang_id2label,
        whisper=whisper,
    )