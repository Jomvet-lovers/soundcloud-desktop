"""Lifecycle задачи: heartbeat + hard timeout, JetStream ack / nak, core-NATS reply."""
import asyncio
import json
import logging
import time
from typing import Awaitable, Callable

from nats.aio.msg import Msg

from ..config import HARD_TIMEOUT_SEC, HEARTBEAT_SEC

log = logging.getLogger(__name__)


async def _heartbeat(msg: Msg, stop: asyncio.Event) -> None:
    """Каждые HEARTBEAT_SEC шлёт +WPI (in_progress), сбрасывая ack_wait."""
    try:
        while not stop.is_set():
            try:
                await asyncio.wait_for(stop.wait(), timeout=HEARTBEAT_SEC)
            except asyncio.TimeoutError:
                await msg.in_progress()
    except Exception as e:
        log.debug(f"heartbeat stopped: {e}")


async def run_with_lifecycle(
    msg: Msg,
    handler: Callable[[dict], Awaitable[dict | None]],
    tag: str,
) -> None:
    """JetStream work-queue: успех → ack; ошибка/таймаут → nak (пойдёт другому воркеру)."""
    try:
        payload = json.loads(msg.data.decode())
    except Exception as e:
        log.error(f"{tag} bad payload: {e}")
        await msg.term()
        return

    stop = asyncio.Event()
    hb_task = asyncio.create_task(_heartbeat(msg, stop))

    try:
        await asyncio.wait_for(handler(payload), timeout=HARD_TIMEOUT_SEC)
        await msg.ack()
    except asyncio.TimeoutError:
        log.warning(f"{tag} hard timeout {HARD_TIMEOUT_SEC}s — nak")
        await msg.nak(delay=0)
    except Exception as e:
        log.error(f"{tag} failed: {e}")
        await msg.nak(delay=5)
    finally:
        stop.set()
        try:
            await hb_task
        except Exception:
            pass


async def _reply(nc, reply_subject: str | None, body: dict) -> None:
    if not reply_subject:
        return
    await nc.publish(reply_subject, json.dumps(body).encode())


async def run_rpc_msg(
    msg: Msg,
    handler: Callable[[str, dict], Awaitable[dict | None]],
    tag: str,
    nc,
) -> None:
    """
    JetStream RPC: backend публикует с заголовком `Nats-Reply-To`, воркер отвечает
    через core NATS и ack'ает JS-сообщение. Ошибка хендлера → reply {ok:false} + ack
    (повторов не будет — backend сам решит что делать).
    """
    hdrs = msg.headers or {}
    reply_subject = hdrs.get("X-Reply-To") or hdrs.get("Nats-Reply-To")
    if not reply_subject:
        log.warning(f"{tag} no reply header; headers={dict(hdrs) if hdrs else 'None'}")
    try:
        payload = json.loads(msg.data.decode())
    except Exception as e:
        log.error(f"{tag} bad payload: {e}")
        await _reply(nc, reply_subject, {"ok": False, "error": f"bad payload: {e}"})
        await msg.term()
        return

    subject = msg.subject
    log.info(f"{tag} received {subject}")
    stop = asyncio.Event()
    hb_task = asyncio.create_task(_heartbeat(msg, stop))
    started = time.monotonic()

    try:
        result = await asyncio.wait_for(handler(subject, payload), timeout=HARD_TIMEOUT_SEC)
        await _reply(nc, reply_subject, {"ok": True, "data": result})
        await msg.ack()
        log.info(f"{tag} done {subject} in {time.monotonic() - started:.2f}s")
    except asyncio.TimeoutError:
        log.warning(f"{tag} hard timeout {subject} ({HARD_TIMEOUT_SEC}s)")
        await _reply(nc, reply_subject, {"ok": False, "error": "timeout"})
        await msg.ack()
    except Exception as e:
        log.error(f"{tag} failed {subject} after {time.monotonic() - started:.2f}s: {e}")
        await _reply(nc, reply_subject, {"ok": False, "error": str(e)})
        await msg.ack()
    finally:
        stop.set()
        try:
            await hb_task
        except Exception:
            pass
