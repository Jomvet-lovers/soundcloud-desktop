"""Two-tower retrieval model — нейронный fusion поверх косинусных сходств.

Архитектура:
  Input: 8-фичевый вектор на пару (user, track):
    [collab_cos, mert_cos, clap_cos, lyrics_cos, log1p_plays, lang_match,
     novelty, recency]
  Шапка: MLP 8 -> 32 -> 16 -> 1, sigmoid выход.

Хэндлеры:
  - train.two_tower.new (JetStream): обучение MLP на (label, features).
  - ai.rpc.two_tower_score (RPC): батч-инференс. Fallback на линейный score
    если модели нет.
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

MODEL_PATH = os.environ.get("TWO_TOWER_MODEL_PATH", "/tmp/two_tower.pt")
N_FEATURES = 8
FALLBACK_WEIGHTS = np.array(
    [0.5, 0.25, 0.10, 0.10, 0.0, 0.05, 0.0, 0.0], dtype=np.float32
)

_model: Any = None
_model_mtime: float = 0.0
_model_lock = threading.Lock()


def _build_mlp():
    import torch
    import torch.nn as nn

    class TwoTowerMLP(nn.Module):
        def __init__(self, in_dim: int = N_FEATURES):
            super().__init__()
            self.net = nn.Sequential(
                nn.Linear(in_dim, 32),
                nn.ReLU(),
                nn.Dropout(0.1),
                nn.Linear(32, 16),
                nn.ReLU(),
                nn.Linear(16, 1),
            )

        def forward(self, x):
            return self.net(x).squeeze(-1)

    return TwoTowerMLP()


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
            model = _build_mlp()
            state = torch.load(MODEL_PATH, map_location=DEVICE)
            model.load_state_dict(state)
            model.train(False)
            model.to(DEVICE)
            _model = model
            _model_mtime = mtime
            log.info(f"[two_tower] loaded model {MODEL_PATH} on {DEVICE}")
            return _model
        except Exception as e:
            log.warning(f"[two_tower] failed to load {MODEL_PATH}: {e}")
            return None


def _fallback_score(features: np.ndarray) -> np.ndarray:
    return features @ FALLBACK_WEIGHTS[: features.shape[1]]


async def score(models, payload: dict) -> dict:
    raw = payload.get("features") or []
    if not raw:
        return {"scores": []}
    features = np.asarray(raw, dtype=np.float32)
    if features.ndim != 2:
        raise ValueError(f"features must be 2D, got shape={features.shape}")

    model = _maybe_load_model()
    if model is None:
        return {"scores": _fallback_score(features).tolist(), "fallback": True}

    try:
        import torch
        with torch.no_grad():
            t = torch.from_numpy(features).to(DEVICE)
            logits = model(t)
            probs = torch.sigmoid(logits).cpu().numpy()
        return {"scores": probs.astype(np.float32).tolist()}
    except Exception as e:
        log.warning(f"[two_tower] predict failed, fallback: {e}")
        return {"scores": _fallback_score(features).tolist(), "fallback": True}


def _train_mlp(features: np.ndarray, labels: np.ndarray, epochs: int = 30) -> tuple[Any, dict]:
    import torch
    import torch.nn as nn
    from torch.utils.data import DataLoader, TensorDataset

    device = torch.device(DEVICE)
    model = _build_mlp().to(device)
    optimizer = torch.optim.Adam(model.parameters(), lr=1e-3, weight_decay=1e-5)
    loss_fn = nn.BCEWithLogitsLoss()

    ds = TensorDataset(
        torch.from_numpy(features.astype(np.float32)),
        torch.from_numpy(labels.astype(np.float32)),
    )
    loader = DataLoader(ds, batch_size=256, shuffle=True)
    losses: list[float] = []
    for _ in range(epochs):
        ep_loss = 0.0
        for xb, yb in loader:
            xb = xb.to(device)
            yb = yb.to(device)
            optimizer.zero_grad()
            logits = model(xb)
            loss = loss_fn(logits, yb)
            loss.backward()
            optimizer.step()
            ep_loss += float(loss.item()) * xb.size(0)
        ep_loss /= len(ds)
        losses.append(ep_loss)
    model.train(False)
    info = {
        "n_examples": int(features.shape[0]),
        "n_features": int(features.shape[1]),
        "epochs": epochs,
        "final_loss": float(losses[-1]) if losses else 0.0,
        "device": str(device),
    }
    return model.cpu(), info


async def handle(payload: dict, models, qdrant, nc) -> None:
    examples = payload.get("examples") or []
    if len(examples) < 200:
        log.warning(f"[two_tower.train] too few examples ({len(examples)}), skip")
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_TWO_TOWER,
            json.dumps({"trained": False, "reason": "too_few", "n": len(examples)}).encode(),
        )
        return

    features_list: list[list[float]] = []
    labels_list: list[float] = []
    for ex in examples:
        feats = ex.get("features") or []
        if len(feats) != N_FEATURES:
            continue
        features_list.append(feats)
        labels_list.append(float(ex.get("label", 0.0)))
    if not features_list:
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_TWO_TOWER,
            json.dumps({"trained": False, "reason": "empty"}).encode(),
        )
        return

    features = np.asarray(features_list, dtype=np.float32)
    labels = np.asarray(labels_list, dtype=np.float32)

    log.info(f"[two_tower.train] starting: n={features.shape[0]} features={features.shape[1]}")
    t0 = time.monotonic()
    model, info = _train_mlp(features, labels, epochs=int(payload.get("epochs", 30)))
    info["train_sec"] = round(time.monotonic() - t0, 2)
    info["trained"] = True

    try:
        import torch
        os.makedirs(os.path.dirname(MODEL_PATH) or ".", exist_ok=True)
        torch.save(model.state_dict(), MODEL_PATH)
        info["model_bytes"] = os.path.getsize(MODEL_PATH)
    except Exception as e:
        log.error(f"[two_tower.train] save failed: {e}")
        info["trained"] = False
        info["error"] = str(e)

    global _model, _model_mtime
    if info["trained"]:
        try:
            import torch
            model.to(DEVICE)
        except Exception:
            pass
        _model = model
        _model_mtime = os.path.getmtime(MODEL_PATH)

    log.info(f"[two_tower.train] done {info}")
    await nc.publish(subj.SUBJECT_DONE_TRAIN_TWO_TOWER, json.dumps(info).encode())
