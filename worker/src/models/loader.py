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


def load_all() -> Models:
    log.info(f"Worker device: {DEVICE} (fp16={USE_FP16})")

    log.info("Loading MuQ (OpenMuQ/MuQ-large-msd-iter)...")
    muq = _prepare(MuQ.from_pretrained("OpenMuQ/MuQ-large-msd-iter"))

    log.info("Loading MuQ-MuLan (OpenMuQ/MuQ-MuLan-large)...")
    mulan = _prepare(MuQMuLan.from_pretrained("OpenMuQ/MuQ-MuLan-large"))

    log.info("Loading bge-m3...")
    st_extra = {"model_kwargs": {"torch_dtype": torch.float16}} if USE_FP16 else {}
    lyrics_embed = SentenceTransformer("BAAI/bge-m3", device=DEVICE, **st_extra)

    log.info("Loading xlm-roberta language detector...")
    lang_name = "papluca/xlm-roberta-base-language-detection"
    lang_tokenizer = AutoTokenizer.from_pretrained(lang_name)
    lang_model = _prepare(AutoModelForSequenceClassification.from_pretrained(lang_name))

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
        lang_id2label=lang_model.config.id2label,
        whisper=whisper,
    )