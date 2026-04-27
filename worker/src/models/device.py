"""Auto-detect CUDA / CPU + fp16 флаг."""
import logging

import torch

from ..config import FORCED_DEVICE

log = logging.getLogger(__name__)


def _resolve_device() -> str:
    if FORCED_DEVICE in {"cpu", "cuda"}:
        return FORCED_DEVICE
    return "cuda" if torch.cuda.is_available() else "cpu"


DEVICE = _resolve_device()
USE_FP16 = DEVICE == "cuda"
