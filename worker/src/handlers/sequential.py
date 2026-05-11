"""Sequential next-track predictor — GRU поверх MERT-векторов сессии.

Architecture:
  Input: последние N MERT-векторов (1024-dim) сессии юзера.
  GRU(1024 -> 256, 1 layer) -> Linear(256, 1024).
  Output: предсказанный "следующий" MERT-вектор. Backend ищет ближайшие
  в TRACKS_MERT.

Handlers:
  - train.sequential.new: обучение на (session_vectors, target_next_vec).
  - ai.rpc.sequential_predict: RPC батч-инференс.
"""
import json
import logging
import os
import threading
import time
from typing import Any

import numpy as np

from .. import subjects as subj
from ..models.device import DEVICE

log = logging.getLogger(__name__)

MODEL_PATH = os.environ.get("SEQUENTIAL_MODEL_PATH", "/tmp/sequential.pt")
INPUT_DIM = 1024
HIDDEN_DIM = 256
MAX_HISTORY = 20

_model: Any = None
_model_mtime: float = 0.0
_model_lock = threading.Lock()


def _build_gru():
    import torch
    import torch.nn as nn

    class SeqPredictor(nn.Module):
        def __init__(self, in_dim: int = INPUT_DIM, hidden: int = HIDDEN_DIM):
            super().__init__()
            self.gru = nn.GRU(in_dim, hidden, num_layers=1, batch_first=True)
            self.proj = nn.Linear(hidden, in_dim)

        def forward(self, x):
            out, _h = self.gru(x)
            last = out[:, -1, :]
            return self.proj(last)

    return SeqPredictor()


def _maybe_load_model():
    global _model, _model_mtime
    if not os.path.exists(MODEL_PATH):
        return None
    try:
        mtime = os.path.getmtime(MODEL_PATH)
    except OSError:
        return None
    if _model is not None and mtime <= _model_mtime:
        return _model
    with _model_lock:
        if _model is not None and mtime <= _model_mtime:
            return _model
        try:
            import torch
            model = _build_gru()
            state = torch.load(MODEL_PATH, map_location=DEVICE)
            model.load_state_dict(state)
            model.train(False)
            model.to(DEVICE)
            _model = model
            _model_mtime = mtime
            log.info(f"[sequential] loaded model {MODEL_PATH} on {DEVICE}")
            return _model
        except Exception as e:
            log.warning(f"[sequential] load failed: {e}")
            return None


def _normalize(v: np.ndarray) -> np.ndarray:
    n = np.linalg.norm(v, axis=-1, keepdims=True)
    n = np.maximum(n, 1e-8)
    return v / n


def _fallback_predict(history: np.ndarray) -> np.ndarray:
    if history.shape[0] == 0:
        return np.zeros(INPUT_DIM, dtype=np.float32)
    weights = np.exp(np.linspace(-1.0, 0.0, history.shape[0]))
    weights /= weights.sum()
    avg = (history * weights[:, None]).sum(axis=0)
    return _normalize(avg.astype(np.float32))


async def predict(models, payload: dict) -> dict:
    sessions = payload.get("sessions") or []
    if not sessions:
        return {"vectors": []}

    out: list[list[float]] = []
    model = _maybe_load_model()

    if model is None:
        for hist in sessions:
            arr = np.asarray(hist, dtype=np.float32)
            if arr.ndim != 2:
                out.append([0.0] * INPUT_DIM)
                continue
            arr = arr[-MAX_HISTORY:]
            out.append(_fallback_predict(arr).tolist())
        return {"vectors": out, "fallback": True}

    try:
        import torch
        with torch.no_grad():
            for hist in sessions:
                arr = np.asarray(hist, dtype=np.float32)
                if arr.ndim != 2 or arr.shape[1] != INPUT_DIM or arr.shape[0] == 0:
                    out.append([0.0] * INPUT_DIM)
                    continue
                arr = arr[-MAX_HISTORY:]
                t = torch.from_numpy(arr).unsqueeze(0).to(DEVICE)
                pred = model(t).squeeze(0).cpu().numpy()
                out.append(_normalize(pred.astype(np.float32)).tolist())
        return {"vectors": out}
    except Exception as e:
        log.warning(f"[sequential] predict failed, fallback: {e}")
        return {
            "vectors": [
                _fallback_predict(np.asarray(h, dtype=np.float32)).tolist() for h in sessions
            ],
            "fallback": True,
        }


def _train(sessions: list, epochs: int = 10) -> tuple[Any, dict]:
    import torch
    import torch.nn as nn

    device = torch.device(DEVICE)
    model = _build_gru().to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=1e-3, weight_decay=1e-5)
    loss_fn = nn.MSELoss()

    inputs: list[np.ndarray] = []
    targets: list[np.ndarray] = []
    for seq in sessions:
        arr = np.asarray(seq, dtype=np.float32)
        if arr.ndim != 2 or arr.shape[0] < 2 or arr.shape[1] != INPUT_DIM:
            continue
        arr = arr[-MAX_HISTORY:]
        inputs.append(arr[:-1])
        targets.append(arr[-1])
    if not inputs:
        return model.cpu(), {"trained": False, "reason": "empty"}

    pad_len = max(a.shape[0] for a in inputs)
    padded = np.zeros((len(inputs), pad_len, INPUT_DIM), dtype=np.float32)
    for i, a in enumerate(inputs):
        padded[i, : a.shape[0]] = a
    tgt = np.stack(targets)

    x_t = torch.from_numpy(padded).to(device)
    y_t = torch.from_numpy(tgt).to(device)

    losses: list[float] = []
    batch_size = 64
    n = x_t.size(0)
    for _ in range(epochs):
        perm = torch.randperm(n)
        ep_loss = 0.0
        for i in range(0, n, batch_size):
            idx = perm[i : i + batch_size]
            xb = x_t[idx]
            yb = y_t[idx]
            optimizer.zero_grad()
            pred = model(xb)
            pred_n = pred / (pred.norm(dim=-1, keepdim=True) + 1e-8)
            yb_n = yb / (yb.norm(dim=-1, keepdim=True) + 1e-8)
            loss = loss_fn(pred_n, yb_n)
            loss.backward()
            optimizer.step()
            ep_loss += float(loss.item()) * xb.size(0)
        ep_loss /= n
        losses.append(ep_loss)
    model.train(False)
    info = {
        "trained": True,
        "n_sequences": n,
        "epochs": epochs,
        "final_loss": float(losses[-1]) if losses else 0.0,
        "device": str(device),
    }
    return model.cpu(), info


async def handle(payload: dict, models, qdrant, nc) -> None:
    sessions = payload.get("sessions") or []
    if len(sessions) < 20:
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_SEQUENTIAL,
            json.dumps({"trained": False, "reason": "too_few", "n": len(sessions)}).encode(),
        )
        return

    log.info(f"[sequential.train] starting: sessions={len(sessions)}")
    t0 = time.monotonic()
    model, info = _train(sessions, epochs=int(payload.get("epochs", 10)))
    info["train_sec"] = round(time.monotonic() - t0, 2)

    if info.get("trained"):
        try:
            import torch
            os.makedirs(os.path.dirname(MODEL_PATH) or ".", exist_ok=True)
            torch.save(model.state_dict(), MODEL_PATH)
            info["model_bytes"] = os.path.getsize(MODEL_PATH)
            global _model, _model_mtime
            try:
                model.to(DEVICE)
            except Exception:
                pass
            _model = model
            _model_mtime = os.path.getmtime(MODEL_PATH)
        except Exception as e:
            log.error(f"[sequential.train] save failed: {e}")
            info["trained"] = False
            info["error"] = str(e)

    log.info(f"[sequential.train] done {info}")
    await nc.publish(subj.SUBJECT_DONE_TRAIN_SEQUENTIAL, json.dumps(info).encode())
