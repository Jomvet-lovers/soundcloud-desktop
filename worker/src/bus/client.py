"""Подключение к NATS."""
import logging

import nats
from nats.aio.client import Client as NATSClient

from ..config import NATS_URL

log = logging.getLogger(__name__)


async def connect() -> NATSClient:
    nc = await nats.connect(
        servers=[NATS_URL],
        name="worker",
        reconnect_time_wait=2,
        max_reconnect_attempts=-1,
        allow_reconnect=True,
    )
    log.info(f"NATS connected → {NATS_URL}")
    return nc
