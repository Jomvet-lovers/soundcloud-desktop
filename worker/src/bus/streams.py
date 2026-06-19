"""JetStream: ensure_stream / ensure_consumer.

Стримы — собственность backend (на приватном NATS) и брокера (на публичном,
см. infra public-workers/broker/init-streams.sh). Воркер их только ИСПОЛЬЗУЕТ:
сначала ЧИТАЕТ (stream_info) и биндится к готовому; add_stream зовёт ТОЛЬКО если
стрима реально нет И есть права (доверенная нода — первый создатель). На публичной
ноде стрим всегда предсоздан → один быстрый INFO, без заведомо-денимого add_stream
(тот спамил ERROR permissions violation и висел по 5с на таймаут запроса — ~40с
старта на 8 стримов). Так один образ живёт и на trusted-, и на untrusted-ноде без
выдачи публичным нодам прав STREAM.CREATE/UPDATE.
"""
import asyncio
import logging

from nats.errors import NoRespondersError
from nats.errors import TimeoutError as NATSTimeoutError
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

# Запрос без ответа: сервер МОЛЧА дропает deny (permissions violation) — клиент
# видит таймаут/no-responders, НЕ ошибку прав. На живом коннекте (connect() уже
# прошёл) это перм-граница (публичная нода вне лейна), а не падение NATS. Реальный
# обрыв даёт ConnectionClosedError (другой класс) → пробрасывается в ретрай.
_DENIED = (NATSTimeoutError, NoRespondersError, asyncio.TimeoutError)

log = logging.getLogger(__name__)


class StreamUnavailable(Exception):
    """Стрим отсутствует, и создать его мы не можем (нет прав STREAM.CREATE).

    Признак того, что этот лейн не обслуживается на данном NATS (публичная нода
    вне бриджуемых брокером лейнов). Вызывающий должен ОТКЛЮЧИТЬ лейн, а не
    ронять/ретраить весь воркер. Отличается от сетевой ошибки/таймаута тем, что
    отсутствие стрима ПОДТВЕРЖДЕНО (stream_info вернул NotFound)."""


async def _ensure_stream(js: JetStreamContext, cfg: StreamConfig) -> None:
    # INFO-first: на публичной ноде стрим предсоздан — читаем и биндимся, БЕЗ
    # заведомо-денимого add_stream (он давал ERROR permissions violation + 5с
    # таймаут запроса на каждый стрим). add_stream только если стрима реально нет.
    try:
        await js.stream_info(cfg.name)
        return  # существует, управляется извне (backend/брокер) — используем как есть
    except NotFoundError:
        pass  # стрима нет — создаём ниже (доверенная нода с правами CREATE)
    except _DENIED as denied:
        # INFO задеймлен (нет прав STREAM.INFO на этот стрим) → перм-граница:
        # лейн недоступен этой ноде. Отключаем, НЕ ретраим вечно.
        raise StreamUnavailable(cfg.name) from denied
    try:
        await js.add_stream(config=cfg)
    except _DENIED as denied:
        # INFO дал NotFound, но CREATE задеймлен (рассинхрон прав/гонка) → недоступен.
        raise StreamUnavailable(cfg.name) from denied
    except Exception as e:
        msg = str(e).lower()
        if "already in use" in msg or "stream name already" in msg:
            # Гонка создателей: кто-то завёл стрим между нашими INFO и CREATE —
            # приводим к нашему конфигу.
            await js.update_stream(config=cfg)
            return
        raise  # реальная ошибка (обрыв NATS и т.п.) → внешний ретрай с backoff


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
        return  # уже есть (предсоздан брокером/backend) — биндимся к нему
    except NotFoundError:
        pass  # ещё не создан — создаём ниже (доверенная нода с правами)
    except _DENIED as denied:
        # нет прав на CONSUMER API для этого стрима → лейн нам недоступен.
        raise StreamUnavailable(f"{stream}/{durable}") from denied
    except Exception as e:
        log.debug(f"consumer_info {stream}/{durable}: {e}")
    try:
        await js.add_consumer(stream, config=cfg)
    except _DENIED as denied:
        # consumer отсутствует и создать не можем (нет прав CREATE) → лейн недоступен.
        raise StreamUnavailable(f"{stream}/{durable}") from denied
