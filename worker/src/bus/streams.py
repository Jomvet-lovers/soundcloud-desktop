"""JetStream: ensure_stream / ensure_consumer. Стримы создаёт backend, воркер добавляет consumer'ы."""
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


async def ensure_work_queue_stream(
    js: JetStreamContext, name: str, subjects: list[str]
) -> None:
    cfg = StreamConfig(
        name=name,
        subjects=subjects,
        retention=RetentionPolicy.WORK_QUEUE,
        storage=StorageType.FILE,
        max_age=24 * 60 * 60,
    )
    try:
        await js.add_stream(config=cfg)
    except Exception as e:
        if "already in use" in str(e) or "stream name already" in str(e):
            await js.update_stream(config=cfg)
        else:
            raise


async def ensure_limits_stream(
    js: JetStreamContext, name: str, subjects: list[str]
) -> None:
    cfg = StreamConfig(
        name=name,
        subjects=subjects,
        retention=RetentionPolicy.LIMITS,
        storage=StorageType.FILE,
        max_age=60 * 60,
    )
    try:
        await js.add_stream(config=cfg)
    except Exception as e:
        if "already in use" in str(e) or "stream name already" in str(e):
            await js.update_stream(config=cfg)
        else:
            raise


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
