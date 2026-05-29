"""Сбор env-переменных в одном месте — чтобы не читать os.environ по всему коду."""
import os

NATS_URL = os.environ["NATS_URL"]

HEARTBEAT_SEC = int(os.environ.get("TASK_HEARTBEAT_SEC", "10"))
HARD_TIMEOUT_SEC = int(os.environ.get("TASK_HARD_TIMEOUT_SEC", "120"))
# Транскрайб (demucs + whisper на полном треке) легально идёт минуты, а не
# секунды — общий 120s hard-timeout его рубил бы и слал в бесконечный ретрай.
# Клиента, который ждёт, нет (фон), поэтому даём щедрый отдельный лимит.
TRANSCRIBE_HARD_TIMEOUT_SEC = int(os.environ.get("TRANSCRIBE_HARD_TIMEOUT_SEC", "1800"))

FORCED_DEVICE = os.environ.get("WORKER_DEVICE", "").lower().strip()

MINI_MODEL = os.environ.get("MINI_MODEL", "google/gemma-4-E2B-it")
WHISPER_MODEL = os.environ.get("WHISPER_MODEL", "base")
WHISPER_COMPUTE = os.environ.get("WHISPER_COMPUTE", "").strip()
DEMUCS_MODEL = os.environ.get("DEMUCS_MODEL", "htdemucs")

# Максимальная длительность аудио, которую отправляем в MuQ/MuLan embed.
# Attention в MuQ — O(T²) по timesteps, на 10-минутном треке это уже ~9 GB
# transient VRAM. 300 сек (5 мин) даёт ~2 GB peak и более чем достаточно для
# жанра/настроения трека (MuQ всё равно усредняет по времени).
# 0 = без ограничения.
MAX_EMBED_DURATION_SEC = int(os.environ.get("MAX_EMBED_DURATION_SEC", "300"))


def _parse_concurrency(raw: str) -> int | dict[str, int]:
    """
    Парсит WORKER_CONCURRENCY:
      - ""       → 1 (глобальный Semaphore(1), дефолт = старое поведение)
      - "N"      → N (глобальный Semaphore(N), общий на все типы)
      - "k=v,…"  → {tag: N} (свой семафор на каждый тип; отсутствующие тэги = 1)
                   Значение 0 для тэга = полное отключение (consumer не
                   регистрируется, связанные модели не грузятся).

    Известные тэги: ai, audio, lyrics, collab, quality.
    """
    raw = (raw or "").strip()
    if not raw:
        return 1
    if "=" not in raw:
        try:
            return max(1, int(raw))
        except ValueError:
            return 1
    out: dict[str, int] = {}
    for part in raw.split(","):
        part = part.strip()
        if not part or "=" not in part:
            continue
        k, _, v = part.partition("=")
        k = k.strip().lower()
        try:
            n = max(0, int(v.strip()))
        except ValueError:
            continue
        if k:
            out[k] = n
    return out or 1


WORKER_CONCURRENCY = _parse_concurrency(os.environ.get("WORKER_CONCURRENCY", ""))
