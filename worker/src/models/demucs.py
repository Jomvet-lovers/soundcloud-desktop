"""Demucs — ленивая загрузка. Нужен только для отделения вокала перед Whisper."""
import logging

from ..config import DEMUCS_MODEL
from .device import DEVICE, USE_FP16
from .registry import Models

log = logging.getLogger(__name__)


def _load_demucs():
    try:
        from demucs.pretrained import get_model as demucs_get_model
    except ImportError:
        log.warning("demucs not installed, vocal separation disabled")
        return None
    log.info(f"Loading Demucs ({DEMUCS_MODEL})...")
    model = demucs_get_model(name=DEMUCS_MODEL)
    model.to(DEVICE)
    for sub in getattr(model, "models", [model]):
        sub.requires_grad_(False)
        set_inference = getattr(sub, "eval", None)
        if callable(set_inference):
            set_inference()
        if USE_FP16 and hasattr(sub, "half"):
            sub.half()
    log.info("Demucs loaded.")
    return model


def ensure_demucs(models: Models):
    """Лениво грузит demucs при первом обращении. None если пакет не установлен."""
    if models.demucs is not None or models.demucs_tried:
        return models.demucs
    models.demucs_tried = True
    models.demucs = _load_demucs()
    return models.demucs
