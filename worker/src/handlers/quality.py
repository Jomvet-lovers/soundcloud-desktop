"""Quality classifier — оценка вероятности что трек качественный музыкальный контент.

Architecture:
  Logistic regression на компактных фичах (10):
    [mert_mean, mert_std, clap_mean, clap_std, log1p_plays, log1p_likes,
     duration_min, title_len_norm, has_genre, is_preview_title]
  sklearn pipeline. Сохранение через joblib.

Handlers:
  - train.quality.new: на размеченных лейблах.
  - ai.rpc.quality_score: батч-инференс.
"""
import json
import logging
import os
import threading
import time
from typing import Any

import numpy as np

from .. import subjects as subj

log = logging.getLogger(__name__)

MODEL_PATH = os.environ.get("QUALITY_MODEL_PATH", "/tmp/quality.joblib")
N_FEATURES = 10

_model: Any = None
_model_mtime: float = 0.0
_model_lock = threading.Lock()


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
            import joblib
            _model = joblib.load(MODEL_PATH)
            _model_mtime = mtime
            log.info(f"[quality] loaded model {MODEL_PATH}")
            return _model
        except Exception as e:
            log.warning(f"[quality] load failed: {e}")
            return None


def _fallback_score(features: np.ndarray) -> np.ndarray:
    if features.shape[0] == 0 or features.shape[1] < N_FEATURES:
        return np.zeros(features.shape[0], dtype=np.float32)
    log_plays = features[:, 4]
    log_likes = features[:, 5]
    duration_min = features[:, 6]
    is_preview = features[:, 9]
    score = (
        0.4 * np.tanh(log_plays / 6.0)
        + 0.3 * np.tanh(log_likes / 4.0)
        + 0.2 * (1.0 - np.clip(np.abs(duration_min - 3.5) / 5.0, 0.0, 1.0))
        + 0.1 * (1.0 - is_preview)
    )
    return np.clip(score, 0.0, 1.0).astype(np.float32)


async def score(models, payload: dict) -> dict:
    raw = payload.get("features") or []
    if not raw:
        return {"scores": []}
    features = np.asarray(raw, dtype=np.float32)
    if features.ndim != 2 or features.shape[1] != N_FEATURES:
        return {"scores": _fallback_score(features).tolist(), "fallback": True}

    clf = _maybe_load_model()
    if clf is None:
        return {"scores": _fallback_score(features).tolist(), "fallback": True}

    try:
        probs = clf.predict_proba(features)[:, 1]
        return {"scores": probs.astype(np.float32).tolist()}
    except Exception as e:
        log.warning(f"[quality] predict failed, fallback: {e}")
        return {"scores": _fallback_score(features).tolist(), "fallback": True}


def _train(features: np.ndarray, labels: np.ndarray) -> tuple[Any, dict]:
    from sklearn.linear_model import LogisticRegression
    from sklearn.pipeline import Pipeline
    from sklearn.preprocessing import StandardScaler

    clf = Pipeline(
        [
            ("scaler", StandardScaler()),
            ("lr", LogisticRegression(max_iter=1000, class_weight="balanced", C=1.0)),
        ]
    )
    clf.fit(features, labels)
    train_score = clf.score(features, labels)
    info = {
        "trained": True,
        "n_examples": int(features.shape[0]),
        "n_positive": int((labels >= 0.5).sum()),
        "train_accuracy": float(train_score),
    }
    return clf, info


async def handle(payload: dict, models, qdrant, nc) -> None:
    examples = payload.get("examples") or []
    if len(examples) < 100:
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_QUALITY,
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
            subj.SUBJECT_DONE_TRAIN_QUALITY,
            json.dumps({"trained": False, "reason": "empty"}).encode(),
        )
        return

    features = np.asarray(features_list, dtype=np.float32)
    labels = np.asarray(labels_list, dtype=np.float32)

    log.info(f"[quality.train] starting: n={features.shape[0]}")
    t0 = time.monotonic()
    clf, info = _train(features, labels)
    info["train_sec"] = round(time.monotonic() - t0, 2)

    try:
        import joblib
        os.makedirs(os.path.dirname(MODEL_PATH) or ".", exist_ok=True)
        joblib.dump(clf, MODEL_PATH)
        info["model_bytes"] = os.path.getsize(MODEL_PATH)
        global _model, _model_mtime
        _model = clf
        _model_mtime = os.path.getmtime(MODEL_PATH)
    except Exception as e:
        log.error(f"[quality.train] save failed: {e}")
        info["trained"] = False
        info["error"] = str(e)

    log.info(f"[quality.train] done {info}")
    await nc.publish(subj.SUBJECT_DONE_TRAIN_QUALITY, json.dumps(info).encode())
