"""Mini LLM (Gemma) — ленивая загрузка. Нужен только в search_queries RPC."""
import logging

import torch
from transformers import AutoModelForCausalLM, AutoTokenizer, BitsAndBytesConfig

from ..config import MINI_MODEL
from .device import DEVICE, USE_FP16
from .registry import Models

log = logging.getLogger(__name__)


def _load_mini():
    log.info(f"Loading mini LLM ({MINI_MODEL})...")
    tokenizer = AutoTokenizer.from_pretrained(MINI_MODEL)
    common_kwargs = {
        "low_cpu_mem_usage": True,
        "attn_implementation": "sdpa",
    }
    if USE_FP16:
        # NF4 4-bit + bfloat16 compute + uint8 storage: минимум VRAM/ОЗУ.
        quant = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=torch.bfloat16,
            bnb_4bit_use_double_quant=True,
            bnb_4bit_quant_storage=torch.uint8,
        )
        try:
            model = AutoModelForCausalLM.from_pretrained(
                MINI_MODEL,
                quantization_config=quant,
                device_map=DEVICE,
                **common_kwargs,
            )
        except TypeError:
            # Старые версии transformers без attn_implementation.
            common_kwargs.pop("attn_implementation", None)
            model = AutoModelForCausalLM.from_pretrained(
                MINI_MODEL,
                quantization_config=quant,
                device_map=DEVICE,
                **common_kwargs,
            )
    else:
        # На CPU — bf16 вместо fp32: вдвое меньше ОЗУ.
        try:
            model = AutoModelForCausalLM.from_pretrained(
                MINI_MODEL, dtype=torch.bfloat16, **common_kwargs
            ).to(DEVICE)
        except TypeError:
            common_kwargs.pop("attn_implementation", None)
            model = AutoModelForCausalLM.from_pretrained(
                MINI_MODEL, dtype=torch.bfloat16, **common_kwargs
            ).to(DEVICE)
    model.requires_grad_(False)
    if hasattr(model, "config"):
        model.config.use_cache = True
    set_inference = getattr(model, "eval", None)
    if callable(set_inference):
        model = set_inference()
    log.info("Mini LLM loaded.")
    return tokenizer, model


def ensure_mini(models: Models):
    """Лениво грузит mini LLM при первом обращении."""
    if models.mini_model is not None:
        return models.mini_tokenizer, models.mini_model
    tokenizer, model = _load_mini()
    models.mini_tokenizer = tokenizer
    models.mini_model = model
    return tokenizer, model
