"""Датакласс Models — общий контейнер для всех загруженных моделей + локи."""
import asyncio
from dataclasses import dataclass, field
from typing import Any

from faster_whisper import WhisperModel
from muq import MuQ, MuQMuLan
from sentence_transformers import SentenceTransformer
from transformers import (
    AutoModelForSequenceClassification,
    AutoTokenizer,
)


@dataclass
class Models:
    muq: MuQ
    mulan: MuQMuLan

    lyrics_embed: SentenceTransformer

    lang_tokenizer: AutoTokenizer
    lang_model: AutoModelForSequenceClassification
    lang_id2label: dict

    whisper: WhisperModel

    # Lazy.
    mini_tokenizer: Any = None
    mini_model: Any = None
    demucs: Any = None
    demucs_tried: bool = False

    mini_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    mulan_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    whisper_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    demucs_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
