"""Подключение к NATS."""
import logging
from urllib.parse import urlsplit, urlunsplit

import nats
from nats.aio.client import Client as NATSClient

from ..config import NATS_URL

log = logging.getLogger(__name__)


def _split_creds(url: str) -> tuple[str, str | None, str | None]:
    # user:pass в URL некоторые nats-клиенты игнорируют — кладём отдельно
    parts = urlsplit(url)
    if not parts.username:
        return url, None, None
    netloc = parts.hostname or ""
    if parts.port:
        netloc = f"{netloc}:{parts.port}"
    clean = urlunsplit((parts.scheme, netloc, parts.path, parts.query, parts.fragment))
    return clean, parts.username, parts.password


async def connect() -> NATSClient:
    host_url, user, password = _split_creds(NATS_URL)
    kwargs = {}
    if user is not None:
        kwargs["user"] = user
    if password is not None:
        kwargs["password"] = password
    nc = await nats.connect(
        servers=[host_url],
        name="worker",
        reconnect_time_wait=2,
        max_reconnect_attempts=-1,
        allow_reconnect=True,
        **kwargs,
    )
    log.info(f"NATS connected → {host_url}")
    return nc
