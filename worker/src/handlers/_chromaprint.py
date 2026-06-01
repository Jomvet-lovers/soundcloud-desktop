"""Raw chromaprint-отпечаток из готового PCM через ctypes к libchromaprint.so.

Зачем: раньше отпечаток считал внешний `fpcalc`, который декодил трек вторым
проходом (audio.py уже декодит его через torchaudio). Тут берём уже декоднутый
PCM и зовём libchromaprint напрямую — декод остаётся один.

Формат вывода байт-в-байт совпадает с `fpcalc -raw`: беззнаковые int32 через
запятую, алгоритм DEFAULT, нативные SR/каналы (chromaprint сам сводит и
ресемплит в 11025). Бэк дедупит по первым 64 символам строки — это и есть
контракт совместимости (проверено на реальном корпусе: prefix64 100%).
"""
import ctypes
import logging

log = logging.getLogger(__name__)

_ALGO_DEFAULT = 1  # CHROMAPRINT_ALGORITHM_DEFAULT (TEST2) — дефолт fpcalc

_lib = None  # None=не пробовали, False=недоступна, иначе CDLL


def _load():
    global _lib
    if _lib is not None:
        return _lib or None
    try:
        lib = ctypes.CDLL("libchromaprint.so.1")
        lib.chromaprint_new.restype = ctypes.c_void_p
        lib.chromaprint_new.argtypes = [ctypes.c_int]
        lib.chromaprint_start.argtypes = [ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
        lib.chromaprint_feed.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int]
        lib.chromaprint_finish.argtypes = [ctypes.c_void_p]
        lib.chromaprint_get_raw_fingerprint.argtypes = [
            ctypes.c_void_p,
            ctypes.POINTER(ctypes.POINTER(ctypes.c_uint32)),
            ctypes.POINTER(ctypes.c_int),
        ]
        lib.chromaprint_dealloc.argtypes = [ctypes.c_void_p]
        lib.chromaprint_free.argtypes = [ctypes.c_void_p]
        _lib = lib
    except OSError as e:
        log.warning(f"[chromaprint] libchromaprint.so.1 unavailable: {e}")
        _lib = False
    return _lib or None


def raw_fingerprint(pcm_s16: bytes, sample_rate: int, channels: int) -> str | None:
    """PCM (int16 interleaved, нативные SR/каналы, первые ≤120с) → raw-fp строкой.

    None — если libchromaprint недоступна, либо клип слишком короткий для
    отпечатка (chromaprint вернул 0 субфингеров). Публикация на стороне audio.py
    всё равно гейтит по truthiness, так что None/«» эквивалентны «нет отпечатка».
    """
    lib = _load()
    if lib is None:
        return None
    ctx = lib.chromaprint_new(_ALGO_DEFAULT)
    if not ctx:
        return None
    try:
        if lib.chromaprint_start(ctx, sample_rate, channels) != 1:
            return None
        if lib.chromaprint_feed(ctx, pcm_s16, len(pcm_s16) // 2) != 1:
            return None
        if lib.chromaprint_finish(ctx) != 1:
            return None
        ptr = ctypes.POINTER(ctypes.c_uint32)()
        size = ctypes.c_int()
        if lib.chromaprint_get_raw_fingerprint(ctx, ctypes.byref(ptr), ctypes.byref(size)) != 1:
            return None
        if size.value <= 0:
            return None
        vals = ",".join(str(ptr[i]) for i in range(size.value))
        lib.chromaprint_dealloc(ptr)
        return vals or None
    finally:
        lib.chromaprint_free(ctx)
