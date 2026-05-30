"""ENCODE: текст запроса → вектор, ответ событием done.encode.

Фоновая work-queue задача (НЕ req/res): backend публикует encode.text.new,
воркер считает вектор когда сможет и публикует done.encode {model, hash, vector}.
Публикуем ТОЛЬКО успешный непустой вектор — пустой/сбойный НЕ публикуем (иначе
backend закэширует негатив и подавит запрос). Сбой модели поднимет исключение →
nak → передоставка (transient восстановится), а 15-мин in-flight лок на стороне
backend сам истечёт. Переиспользует те же модели, что ai-лейн (mulan / bge-m3),
поэтому живёт под тем же тэгом `ai`."""
import json
import logging

from nats.aio.client import Client as NATSClient

from .. import subjects as subj
from ..models import Models
from . import ai

log = logging.getLogger(__name__)


async def handle(payload: dict, models: Models, nc: NATSClient) -> None:
    model = (payload.get("model") or "").strip()
    text = (payload.get("text") or "").strip()
    h = payload.get("hash") or ""
    if not h or not text:
        return  # некуда/нечего считать — тихий ack, без публикации
    if model == "mulan":
        vector = (await ai.encode_text_mulan(models, {"text": text})).get("vector") or []
    elif model == "lyrics":
        vector = (await ai.encode_lyrics_text(models, {"text": text})).get("vector") or []
    else:
        raise ValueError(f"unknown encode model: {model}")
    # Публикуем ТОЛЬКО успешный непустой вектор. Сбой поднимет исключение → nak →
    # передоставка; негатив в кэш не кладём.
    if not vector:
        return
    done = {"model": model, "hash": h, "vector": vector}
    await nc.publish(subj.SUBJECT_DONE_ENCODE, json.dumps(done).encode())
    log.info(f"[encode] {model} hash={h[:8]} dim={len(vector)} done.encode published")
