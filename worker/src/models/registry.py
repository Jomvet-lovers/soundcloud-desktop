"""Датакласс Models — общий контейнер для всех загруженных моделей + локи.

Все поля моделей опциональны (None), потому что воркер может быть запущен
в audio-only / lyrics-only режимах через WORKER_CONCURRENCY=...=0 и
лишние модели не загружаются ради экономии VRAM/RAM. Хэндлеры проверяют
наличие модели через `if models.X is None: raise ...`; в нормальной работе
хэндлер не получит задачу для отключённого тэга — pull-loop не поднят.
"""
import asyncio
from dataclasses import dataclass, field
from typing import Any, Optional

from faster_whisper import WhisperModel
from muq import MuQ, MuQMuLan
from sentence_transformers import SentenceTransformer
from transformers import (
    AutoModelForSequenceClassification,
    AutoTokenizer,
)


@dataclass
class Models:
    muq: Optional[MuQ] = None
    mulan: Optional[MuQMuLan] = None

    lyrics_embed: Optional[SentenceTransformer] = None

    lang_tokenizer: Optional[AutoTokenizer] = None
    lang_model: Optional[AutoModelForSequenceClassification] = None
    lang_id2label: dict = field(default_factory=dict)

    whisper: Optional[WhisperModel] = None

    # Lazy.
    mini_tokenizer: Any = None
    mini_model: Any = None
    demucs: Any = None
    demucs_tried: bool = False

    mini_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    mulan_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    whisper_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
    demucs_lock: asyncio.Lock = field(default_factory=asyncio.Lock)
