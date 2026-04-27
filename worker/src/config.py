"""Сбор env-переменных в одном месте — чтобы не читать os.environ по всему коду."""
import os

NATS_URL = os.environ["NATS_URL"]
QDRANT_URL = os.environ["QDRANT_URL"]
QDRANT_API_KEY = os.environ.get("QDRANT_API_KEY", "") or None

HEARTBEAT_SEC = int(os.environ.get("TASK_HEARTBEAT_SEC", "10"))
HARD_TIMEOUT_SEC = int(os.environ.get("TASK_HARD_TIMEOUT_SEC", "120"))

FORCED_DEVICE = os.environ.get("WORKER_DEVICE", "").lower().strip()

MINI_MODEL = os.environ.get("MINI_MODEL", "google/gemma-4-E2B-it")
WHISPER_MODEL = os.environ.get("WHISPER_MODEL", "base")
WHISPER_COMPUTE = os.environ.get("WHISPER_COMPUTE", "").strip()
DEMUCS_MODEL = os.environ.get("DEMUCS_MODEL", "htdemucs")
