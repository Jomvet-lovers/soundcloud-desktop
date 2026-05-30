"""Раннеры лейнов поверх JetStream pull-consumer'ов. См. worker/AGENTS.md."""
import asyncio
import json
import logging
import time
from dataclasses import dataclass
from typing import Any, Awaitable, Callable

from nats.aio.msg import Msg

from .bus.rpc import _heartbeat, run_rpc_msg, run_with_lifecycle
from .config import HARD_TIMEOUT_SEC

log = logging.getLogger(__name__)


async def run_concurrent_lane(
        js,
        sem: asyncio.Semaphore,
        stream: str,
        durable: str,
        subject: str,
        handler_factory,
        tag: str,
        stop: asyncio.Event,
        *,
        is_rpc: bool,
        nc=None,
        hard_timeout: int | None = None,
) -> None:
    """До N одновременных задач: fetch → spawn task → permit отпускается в его finally."""
    psub = await js.pull_subscribe(subject, durable=durable)
    log.info(f"concurrent-lane {durable} → {subject}")
    err_streak = 0
    inflight: set[asyncio.Task] = set()

    async def _process(msg: Msg) -> None:
        try:
            if is_rpc:
                await run_rpc_msg(msg, handler_factory, tag, nc)
            else:
                await run_with_lifecycle(msg, handler_factory, tag, hard_timeout)
        except asyncio.CancelledError:
            raise
        except Exception as e:
            log.error(f"{tag} task crashed: {e}")
        finally:
            sem.release()

    try:
        while not stop.is_set():
            await sem.acquire()
            try:
                msgs = await psub.fetch(batch=1, timeout=1)
                err_streak = 0
            except asyncio.TimeoutError:
                sem.release()
                continue
            except asyncio.CancelledError:
                sem.release()
                raise
            except Exception as e:
                sem.release()
                if stop.is_set():
                    return
                err_streak += 1
                log.error(f"{tag} fetch failed ({err_streak}): {e}")
                if err_streak >= 5:
                    psub = await _resubscribe(js, psub, subject, durable, tag)
                    err_streak = 0
                try:
                    await asyncio.wait_for(stop.wait(), timeout=1)
                    return
                except asyncio.TimeoutError:
                    continue
            if not msgs:
                sem.release()
                continue
            task = asyncio.create_task(_process(msgs[0]))
            inflight.add(task)
            task.add_done_callback(inflight.discard)
    finally:
        for t in inflight:
            t.cancel()


@dataclass
class _InFlight:
    msg: Msg
    prepared: Any
    payload: dict
    hb_task: asyncio.Task
    hb_stop: asyncio.Event


PrepareFn = Callable[[dict, Any], Awaitable[Any]]
GpuBatchFn = Callable[[Any, list], list]
PublishFn = Callable[[Any, dict, Any], Awaitable[None]]


async def run_batched_lane(
        js,
        models,
        nc,
        *,
        stream: str,
        durable: str,
        subject: str,
        tag: str,
        stop: asyncio.Event,
        prepare: PrepareFn,
        gpu_batch: GpuBatchFn,
        publish: PublishFn,
        publish_skip: PublishFn | None = None,
        fanout: int,
        max_batch: int,
        wait_ms: int,
        hard_timeout: int | None = None,
        gpu_lock: asyncio.Lock | None = None,
) -> None:
    """Фан-аут качалки (prepare) → очередь → один GPU-исполнитель (gpu_batch)."""
    timeout = hard_timeout or HARD_TIMEOUT_SEC
    psub = await js.pull_subscribe(subject, durable=durable)
    queue: asyncio.Queue = asyncio.Queue(maxsize=max(2 * max_batch, fanout + 1))
    log.info(f"batched-lane {durable} → {subject} (fanout={fanout} batch={max_batch})")

    async def _stop_hb(item: _InFlight) -> None:
        item.hb_stop.set()
        await _await_quiet(item.hb_task)

    async def _intake(msg: Msg) -> None:
        try:
            payload = json.loads(msg.data.decode())
        except Exception as e:
            log.error(f"{tag} bad payload: {e}")
            await msg.term()
            return
        hb_stop = asyncio.Event()
        hb = asyncio.create_task(_heartbeat(msg, hb_stop))
        try:
            prepared = await prepare(payload, models)
        except Exception as e:
            log.error(f"{tag} prepare failed: {e}")
            hb_stop.set()
            await _await_quiet(hb)
            await msg.nak(delay=5)
            return
        if prepared is None:
            hb_stop.set()
            await _await_quiet(hb)
            if publish_skip is not None:
                try:
                    await publish_skip(nc, payload, None)
                except Exception as e:
                    log.warning(f"{tag} skip publish failed: {e}")
            await msg.ack()
            return
        await queue.put(_InFlight(msg, prepared, payload, hb, hb_stop))

    async def _fetch_loop() -> None:
        nonlocal psub
        sem = asyncio.Semaphore(fanout)
        prep_tasks: set[asyncio.Task] = set()
        err_streak = 0

        async def _prep(msg: Msg) -> None:
            try:
                await _intake(msg)
            finally:
                sem.release()

        try:
            while not stop.is_set():
                await sem.acquire()
                try:
                    msgs = await psub.fetch(batch=1, timeout=1)
                    err_streak = 0
                except asyncio.TimeoutError:
                    sem.release()
                    continue
                except asyncio.CancelledError:
                    sem.release()
                    raise
                except Exception as e:
                    sem.release()
                    if stop.is_set():
                        return
                    err_streak += 1
                    log.error(f"{tag} fetch failed ({err_streak}): {e}")
                    if err_streak >= 5:
                        psub = await _resubscribe(js, psub, subject, durable, tag)
                        err_streak = 0
                    try:
                        await asyncio.wait_for(stop.wait(), timeout=1)
                        return
                    except asyncio.TimeoutError:
                        continue
                if not msgs:
                    sem.release()
                    continue
                t = asyncio.create_task(_prep(msgs[0]))
                prep_tasks.add(t)
                t.add_done_callback(prep_tasks.discard)
        finally:
            for t in prep_tasks:
                t.cancel()

    async def _run_batch(batch: list[_InFlight]) -> None:
        prepared = [b.prepared for b in batch]
        try:
            if gpu_lock is not None:
                async with gpu_lock:
                    results = await asyncio.wait_for(
                        asyncio.to_thread(gpu_batch, models, prepared), timeout=timeout
                    )
            else:
                results = await asyncio.wait_for(
                    asyncio.to_thread(gpu_batch, models, prepared), timeout=timeout
                )
        except Exception as e:
            if len(batch) == 1:
                log.error(f"{tag} item failed: {e}")
                await batch[0].msg.nak(delay=5)
                await _stop_hb(batch[0])
                return
            log.warning(f"{tag} batch×{len(batch)} failed ({e}); retry per-item")
            for b in batch:
                await _run_batch([b])
            return
        # gpu_batch обязан вернуть по результату на элемент. Короткий возврат
        # иначе тихо проглотил бы хвост батча (ни ack, ни nak) и навсегда оставил
        # бы их heartbeat-таски крутиться → nak'аем весь батч, гасим heartbeats.
        if len(results) != len(batch):
            log.error(f"{tag} gpu_batch returned {len(results)}/{len(batch)} results; naking batch")
            for b in batch:
                await b.msg.nak(delay=5)
                await _stop_hb(b)
            return
        for b, r in zip(batch, results):
            try:
                await publish(nc, b.payload, r)
                await b.msg.ack()
            except Exception as e:
                log.error(f"{tag} publish/ack failed: {e}")
                await b.msg.nak(delay=5)
            finally:
                await _stop_hb(b)

    async def _executor() -> None:
        while not stop.is_set():
            try:
                first = await asyncio.wait_for(queue.get(), timeout=1)
            except asyncio.TimeoutError:
                continue
            batch = [first]
            if max_batch > 1 and wait_ms > 0:
                deadline = time.monotonic() + wait_ms / 1000.0
                while len(batch) < max_batch:
                    remaining = deadline - time.monotonic()
                    if remaining <= 0:
                        break
                    try:
                        batch.append(await asyncio.wait_for(queue.get(), timeout=remaining))
                    except asyncio.TimeoutError:
                        break
            else:
                while len(batch) < max_batch and not queue.empty():
                    batch.append(queue.get_nowait())
            await _run_batch(batch)

    fetcher = asyncio.create_task(_fetch_loop())
    executor = asyncio.create_task(_executor())
    try:
        await stop.wait()
    finally:
        fetcher.cancel()
        executor.cancel()
        await _await_quiet(fetcher)
        await _await_quiet(executor)
        while not queue.empty():
            item = queue.get_nowait()
            item.hb_stop.set()
            await _await_quiet(item.hb_task)


async def _resubscribe(js, psub, subject: str, durable: str, tag: str):
    try:
        await psub.unsubscribe()
    except Exception:
        pass
    try:
        new_psub = await js.pull_subscribe(subject, durable=durable)
        log.info(f"{tag} resubscribed")
        return new_psub
    except Exception as e:
        log.error(f"{tag} resubscribe failed: {e}")
        return psub


async def _await_quiet(task: asyncio.Task) -> None:
    try:
        await task
    except (asyncio.CancelledError, Exception):
        pass
