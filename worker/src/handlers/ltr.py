"""LTR (Learning To Rank) — финальный rerank поверх 4-base scoring.

Два хэндлера:
  - train.ltr.new (JetStream durable): обучение LGBMRanker на исторических
    (user, track, label, features) парах. Модель сохраняется в /tmp/ltr_model.txt,
    в done.train_ltr публикуем размер модели и метрики.
  - ai.rpc.ltr_score (RPC, fast): получает батч feature-векторов, возвращает
    скоры. Модель кешируется в global state, перезагружается с диска если
    mtime новее.

Фичи (синхронизированы с backend/src/ltr/ltr-features.service.ts):
  [collab_cos, mert_cos, clap_cos, lyrics_cos, log1p_playback, language_match]

Если модель ещё не обучена — score() возвращает linear fallback
(сумма cosines, как до LTR), и backend продолжает работать без падений.
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

MODEL_PATH = os.environ.get("LTR_MODEL_PATH", "/tmp/ltr_model.txt")
N_FEATURES = 6
FALLBACK_WEIGHTS = np.array([0.55, 0.20, 0.10, 0.15, 0.0, 0.0], dtype=np.float32)

_model: Any = None
_model_mtime: float = 0.0
_model_lock = threading.Lock()


def _maybe_load_model() -> Any | None:
    """Перезагружает модель если файл изменился. Кеширует mtime."""
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
            import lightgbm as lgb
            booster = lgb.Booster(model_file=MODEL_PATH)
            _model = booster
            _model_mtime = mtime
            log.info(f"[ltr] loaded model {MODEL_PATH} (mtime={mtime})")
            return _model
        except Exception as e:
            log.warning(f"[ltr] failed to load model {MODEL_PATH}: {e}")
            return None


def _fallback_score(features: np.ndarray) -> np.ndarray:
    """Линейная комбинация — fallback пока модель не обучена."""
    return features @ FALLBACK_WEIGHTS[: features.shape[1]]


async def score(models, payload: dict) -> dict:
    """ai.rpc.ltr_score handler — RPC, инференс модели на батче фичей."""
    raw = payload.get("features") or []
    if not raw:
        return {"scores": []}
    features = np.asarray(raw, dtype=np.float32)
    if features.ndim != 2:
        raise ValueError(f"features must be 2D, got shape={features.shape}")

    booster = _maybe_load_model()
    if booster is None:
        scores = _fallback_score(features)
        return {"scores": scores.tolist(), "fallback": True}

    try:
        preds = booster.predict(features, num_iteration=booster.best_iteration)
        return {"scores": np.asarray(preds, dtype=np.float32).tolist()}
    except Exception as e:
        log.warning(f"[ltr] predict failed, fallback: {e}")
        return {"scores": _fallback_score(features).tolist(), "fallback": True}


def _train_lightgbm(
    features: np.ndarray,
    labels: np.ndarray,
    groups: np.ndarray,
    params: dict | None = None,
) -> tuple[Any, dict]:
    import lightgbm as lgb

    p = {
        "objective": "lambdarank",
        "metric": "ndcg",
        "ndcg_eval_at": [5, 10, 20],
        "learning_rate": 0.05,
        "num_leaves": 31,
        "min_data_in_leaf": 20,
        "feature_fraction": 0.9,
        "bagging_fraction": 0.9,
        "bagging_freq": 5,
        "lambda_l2": 0.1,
        "verbose": -1,
    }
    if params:
        p.update(params)

    train_set = lgb.Dataset(features, label=labels, group=groups)
    booster = lgb.train(p, train_set, num_boost_round=200)
    info = {
        "feature_importance": booster.feature_importance(importance_type="gain").tolist(),
        "best_iteration": booster.best_iteration or 200,
        "n_features": int(features.shape[1]),
        "n_examples": int(features.shape[0]),
        "n_groups": int(len(groups)),
    }
    return booster, info


async def handle(payload: dict, models, qdrant, nc) -> None:
    """train.ltr.new handler — обучение LGBMRanker.

    Payload:
      {
        "examples": [{"group": int, "label": float, "features": [f1...fN]}, ...],
        "params": {...}  # опц. lightgbm overrides
      }
    """
    examples = payload.get("examples") or []
    if len(examples) < 200:
        log.warning(f"[ltr.train] too few examples ({len(examples)}), skip")
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_LTR,
            json.dumps({"trained": False, "reason": "too_few_examples", "n": len(examples)}).encode(),
        )
        return

    # Сортировка по group, чтобы LGBM получил последовательные группы.
    examples.sort(key=lambda e: e["group"])

    features_list: list[list[float]] = []
    labels_list: list[float] = []
    group_sizes: list[int] = []
    current_group = None
    current_size = 0
    for ex in examples:
        g = ex["group"]
        if g != current_group:
            if current_group is not None:
                group_sizes.append(current_size)
            current_group = g
            current_size = 0
        feats = ex.get("features") or []
        if len(feats) != N_FEATURES:
            continue
        features_list.append(feats)
        labels_list.append(float(ex.get("label", 0.0)))
        current_size += 1
    if current_size > 0:
        group_sizes.append(current_size)

    if not features_list:
        log.warning("[ltr.train] empty features after filtering")
        await nc.publish(
            subj.SUBJECT_DONE_TRAIN_LTR,
            json.dumps({"trained": False, "reason": "empty_features"}).encode(),
        )
        return

    features = np.asarray(features_list, dtype=np.float32)
    labels = np.asarray(labels_list, dtype=np.float32)
    groups = np.asarray(group_sizes, dtype=np.int32)

    log.info(
        f"[ltr.train] starting: examples={features.shape[0]} groups={groups.shape[0]} "
        f"features={features.shape[1]} avg_group={features.shape[0] / max(groups.shape[0], 1):.1f}"
    )
    t0 = time.monotonic()
    booster, info = _train_lightgbm(features, labels, groups, payload.get("params"))
    train_sec = time.monotonic() - t0

    os.makedirs(os.path.dirname(MODEL_PATH), exist_ok=True)
    booster.save_model(MODEL_PATH)
    size_bytes = os.path.getsize(MODEL_PATH)
    info["train_sec"] = round(train_sec, 2)
    info["model_bytes"] = size_bytes
    info["trained"] = True

    # Сразу подменим in-memory модель, чтобы score() её увидел без задержки.
    global _model, _model_mtime
    _model = booster
    _model_mtime = os.path.getmtime(MODEL_PATH)

    log.info(f"[ltr.train] done in {train_sec:.2f}s model_bytes={size_bytes} info={info}")
    await nc.publish(subj.SUBJECT_DONE_TRAIN_LTR, json.dumps(info).encode())
