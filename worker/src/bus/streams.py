"""JetStream: ensure_stream / ensure_consumer.

Стримы — собственность backend (на приватном NATS) и брокера (на публичном,
см. infra public-workers/broker/init-streams.sh). Воркер их только ИСПОЛЬЗУЕТ:
пробует создать (на доверенной ноде с правами это no-op/создание), а если прав
нет (публичная нода) — убеждается, что стрим уже предсоздан, и работает с ним.
Так один и тот же образ живёт и на trusted-, и на untrusted-ноде без выдачи
публичным нодам прав STREAM.CREATE/UPDATE.
"""
import logging

from nats.js import JetStreamContext
from nats.js.api import (
    AckPolicy,
    ConsumerConfig,
    DeliverPolicy,
    RetentionPolicy,
    StorageType,
    StreamConfig,
)
from nats.js.errors import NotFoundError

log = logging.getLogger(__name__)


class StreamUnavailable(Exception):
    """Стрим отсутствует, и создать его мы не можем (нет прав STREAM.CREATE).

    Признак того, что этот лейн не обслуживается на данном NATS (публичная нода
    вне бриджуемых брокером лейнов). Вызывающий должен ОТКЛЮЧИТЬ лейн, а не
    ронять/ретраить весь воркер. Отличается от сетевой ошибки/таймаута тем, что
    отсутствие стрима ПОДТВЕРЖДЕНО (stream_info вернул NotFound)."""


async def _ensure_stream(js: JetStreamContext, cfg: StreamConfig) -> None:
    try:
        await js.add_stream(config=cfg)
        return
    except Exception as e:
        msg = str(e).lower()
        if "already in use" in msg or "stream name already" in msg:
            # Права есть, стрим уже существует с другим конфигом — приводим к нашему.
            await js.update_stream(config=cfg)
            return
        # add_stream не прошёл по иной причине: либо нет прав CREATE (публичная
        # нода — приходит как permissions violation + таймаут запроса), либо NATS
        # недоступен. Различаем чтением стрима (INFO ноде разрешён).
        try:
            await js.stream_info(cfg.name)
        except NotFoundError:
            # Стрим реально отсутствует, а создать не можем → лейн недоступен.
            raise StreamUnavailable(cfg.name) from e
        # Стрим есть (предсоздан backend'ом/брокером) — просто используем его.
        log.info("stream %s managed externally; not creating", cfg.name)


async def ensure_work_queue_stream(
    js: JetStreamContext, name: str, subjects: list[str]
) -> None:
    await _ensure_stream(
        js,
        StreamConfig(
            name=name,
            subjects=subjects,
            retention=RetentionPolicy.WORK_QUEUE,
            storage=StorageType.FILE,
            max_age=24 * 60 * 60,
        ),
    )


async def ensure_limits_stream(
    js: JetStreamContext, name: str, subjects: list[str]
) -> None:
    await _ensure_stream(
        js,
        StreamConfig(
            name=name,
            subjects=subjects,
            retention=RetentionPolicy.LIMITS,
            storage=StorageType.FILE,
            max_age=60 * 60,
        ),
    )


async def ensure_consumer(
    js: JetStreamContext,
    stream: str,
    durable: str,
    subject: str,
) -> None:
    cfg = ConsumerConfig(
        durable_name=durable,
        ack_policy=AckPolicy.EXPLICIT,
        deliver_policy=DeliverPolicy.ALL,
        ack_wait=30,  # секунды; heartbeat раз в 10с сбрасывает
        max_deliver=5,
        filter_subject=subject,
    )
    try:
        await js.consumer_info(stream, durable)
    except NotFoundError:
        await js.add_consumer(stream, config=cfg)
    except Exception as e:
        log.debug(f"consumer_info {stream}/{durable}: {e}")
        await js.add_consumer(stream, config=cfg)
