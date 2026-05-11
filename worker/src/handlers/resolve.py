"""ai.rpc.resolve_artist — определение реального автора SoundCloud-трека.

LLM-агент с инструментами: web_search, fetch_page, mb_search, genius_search.
Цикл step-by-step: LLM либо просит инструмент, либо отдаёт финальный JSON.
Backend вызывает этот handler как fallback после ISRC/MB/Genius.
"""
import asyncio
import gc
import json
import logging
import os
import re
from typing import Any

import aiohttp
import torch

from ..models import DEVICE, Models, ensure_mini
from . import resolve_tools as tools

EXTERNAL_PROVIDER = (os.environ.get("AI_EXTERNAL_PROVIDER") or "").strip().lower()
EXTERNAL_API_KEY = (os.environ.get("AI_EXTERNAL_API_KEY") or "").strip()
EXTERNAL_MODEL_OPENAI = os.environ.get("AI_EXTERNAL_MODEL_OPENAI") or "gpt-4o-mini"
EXTERNAL_MODEL_ANTHROPIC = os.environ.get("AI_EXTERNAL_MODEL_ANTHROPIC") or "claude-haiku-4-5-20251001"
EXTERNAL_TIMEOUT_SEC = 30

log = logging.getLogger(__name__)

MAX_STEPS = 12
MAX_TRANSCRIPT_CHARS = 24_000
MAX_NEW_TOKENS = 900

PROMPT = """Ты ассистент для разрешения метаданных SoundCloud-треков.

Контекст:
- На SoundCloud массово залиты re-uploads. Аплоадер часто = "nightcore", "vibes",
  "boost", "type beat", какой-нибудь промо-канал — это НЕ автор. Реальный автор обычно
  спрятан в названии трека вида "Real Artist - Real Title".
- Твоя задача: вернуть РЕАЛЬНОГО primary_artist + featured/producers/remixers + альбом если есть.
- Сохраняй оригинальное написание (русский, японский — не транслитерируй).

У тебя есть инструменты для поиска. Вызывай их по одному, пока не получишь
достаточно данных. Затем верни final answer.

# Инструменты

1. web_search: общий веб-поиск через DuckDuckGo.
   args: {{"query": "строка"}}
   возвращает: список {{"url", "title", "snippet"}}

2. fetch_page: загрузить URL и получить очищенный текст страницы (до 3000 символов).
   args: {{"url": "https://..."}}
   возвращает: текст страницы или {{"error": "..."}}

3. mb_search: поиск в MusicBrainz (точная music DB).
   args: {{"query": "Eminem Lose Yourself"}}
   возвращает: список {{"title", "artist", "album", "year", "score"}}

4. genius_search: поиск в Genius (база песен, шире чем MB).
   args: {{"query": "Eminem Lose Yourself"}}
   возвращает: список {{"title", "primary_artist", "featured"}}

5. wikipedia_search: поиск в английской Википедии. Хорош для проверки фактов
   (существует ли исполнитель, к какому жанру относится и т.п.).
   args: {{"query": "Eminem rapper"}}
   возвращает: список {{"title", "snippet", "url"}}

6. discogs_search: дискография (релизы, лейблы, годы). Может вернуть error если
   ключ не настроен — тогда не используй.
   args: {{"query": "Eminem Lose Yourself"}}
   возвращает: список {{"title", "artist", "year", "label", "format"}}

# Протокол ответа

На каждом шаге выводи РОВНО ОДИН JSON. Никаких пояснений, никакого текста до или после.

Запрос инструмента:
{{"action": "tool", "tool": "<имя>", "args": {{...}}}}

Финальный ответ:
{{"action": "answer", "data": {{
  "primary_artist": "имя или null",
  "featured": ["..."],
  "producers": ["..."],
  "remixers": ["..."],
  "album": {{"title": "...", "year": int|null, "primary_artist": "..."|null}} | null,
  "confidence": 0.0..1.0
}}}}

# Стратегия

1. Сначала попробуй mb_search и genius_search с очевидным запросом из тайтла.
2. Если результаты не совпадают с продолжительностью / контекстом — попробуй web_search,
   возможно fetch_page на топовый результат.
3. Если уверенности нет (нашёл много вариантов / ничего не подходит) — confidence ставь низкий
   (< 0.5) или primary_artist=null.
4. Не выдумывай. Лучше null чем галлюцинация.

# Вход

title: {title}
uploader (часто re-uploader, НЕ автор): {uploader}
metadata_artist (что аплоадер сам указал; может быть мусором): {metadata_artist}
duration_ms: {duration_ms}
isrc: {isrc}
description (фрагмент): {description}
tags: {tags}

Начинай. Один JSON на ответ."""


_THINK_RE = re.compile(r"<think>.*?</think>", re.DOTALL | re.IGNORECASE)
_FENCE_RE = re.compile(r"^```(?:json)?\s*|\s*```$", re.MULTILINE)


async def resolve_artist(models: Models, payload: dict) -> dict:
    title = (payload.get("title") or "")[:300]
    uploader = (payload.get("uploader") or "")[:200]
    duration_ms = payload.get("duration_ms")
    isrc = payload.get("isrc") or ""
    metadata_artist = (payload.get("metadata_artist") or "")[:200]
    raw = payload.get("raw") or {}
    description = (
        payload.get("description")
        or raw.get("description")
        or ""
    )[:1500]
    tag_list = (raw.get("tag_list") or "")[:500]

    initial = PROMPT.format(
        title=title,
        uploader=uploader or "(unknown)",
        metadata_artist=metadata_artist or "(none)",
        duration_ms=duration_ms if duration_ms is not None else "(unknown)",
        isrc=isrc or "(none)",
        description=description or "(empty)",
        tags=tag_list or "(none)",
    )

    transcript = [{"role": "user", "content": initial}]

    timeout = aiohttp.ClientTimeout(total=tools.TOOL_TIMEOUT + 4)
    async with aiohttp.ClientSession(timeout=timeout) as http:
        for step in range(MAX_STEPS):
            try:
                raw_out = await _generate(models, transcript)
            except Exception as e:
                log.warning(f"[resolve] generate failed at step {step}: {e}")
                return _empty_answer()

            log.info(f"[resolve] step={step} llm_out={raw_out[:400]!r}")

            try:
                msg = _parse_json(raw_out)
            except Exception as e:
                log.warning(f"[resolve] parse failed: {e}; out={raw_out!r}")
                return _empty_answer()

            action = msg.get("action")
            if action == "answer":
                return _normalize(msg.get("data") or {})

            if action == "tool":
                tool_name = msg.get("tool") or ""
                args = msg.get("args") or {}
                tool_result = await _run_tool(http, tool_name, args)
                transcript.append(
                    {"role": "assistant", "content": json.dumps(msg, ensure_ascii=False)}
                )
                transcript.append(
                    {
                        "role": "user",
                        "content": json.dumps(
                            {"tool_result": tool_result}, ensure_ascii=False
                        ),
                    }
                )
                _trim_transcript(transcript)
                continue

            log.warning(f"[resolve] unknown action: {msg!r}")
            return _empty_answer()

    log.warning("[resolve] step budget exhausted")
    return _empty_answer()


async def _generate(models: Models, transcript: list[dict]) -> str:
    if EXTERNAL_PROVIDER and EXTERNAL_API_KEY:
        try:
            return await _generate_external(transcript)
        except Exception as e:
            log.warning(f"[resolve] external LLM failed, falling back to local: {e}")
    async with models.mini_lock:
        return await asyncio.to_thread(_generate_sync, models, transcript)


async def _generate_external(transcript: list[dict]) -> str:
    timeout = aiohttp.ClientTimeout(total=EXTERNAL_TIMEOUT_SEC)
    async with aiohttp.ClientSession(timeout=timeout) as http:
        if EXTERNAL_PROVIDER == "openai":
            return await _generate_openai(http, transcript)
        if EXTERNAL_PROVIDER == "anthropic":
            return await _generate_anthropic(http, transcript)
        raise ValueError(f"unknown AI_EXTERNAL_PROVIDER: {EXTERNAL_PROVIDER!r}")


async def _generate_openai(http: aiohttp.ClientSession, transcript: list[dict]) -> str:
    body = {
        "model": EXTERNAL_MODEL_OPENAI,
        "messages": transcript,
        "temperature": 0,
        "response_format": {"type": "json_object"},
    }
    headers = {
        "Authorization": f"Bearer {EXTERNAL_API_KEY}",
        "Content-Type": "application/json",
    }
    async with http.post(
        "https://api.openai.com/v1/chat/completions",
        json=body,
        headers=headers,
    ) as r:
        if r.status >= 400:
            raise RuntimeError(f"openai status {r.status}: {await r.text()}")
        data = await r.json()
    return ((data.get("choices") or [{}])[0].get("message") or {}).get("content") or ""


async def _generate_anthropic(http: aiohttp.ClientSession, transcript: list[dict]) -> str:
    system_msg = ""
    msgs = []
    for m in transcript:
        if m.get("role") == "system":
            system_msg = m.get("content") or ""
            continue
        role = "assistant" if m.get("role") == "assistant" else "user"
        msgs.append({"role": role, "content": m.get("content") or ""})
    body = {
        "model": EXTERNAL_MODEL_ANTHROPIC,
        "max_tokens": MAX_NEW_TOKENS,
        "temperature": 0,
        "messages": msgs,
    }
    if system_msg:
        body["system"] = system_msg
    headers = {
        "x-api-key": EXTERNAL_API_KEY,
        "anthropic-version": "2023-06-01",
        "Content-Type": "application/json",
    }
    async with http.post(
        "https://api.anthropic.com/v1/messages",
        json=body,
        headers=headers,
    ) as r:
        if r.status >= 400:
            raise RuntimeError(f"anthropic status {r.status}: {await r.text()}")
        data = await r.json()
    parts = data.get("content") or []
    for p in parts:
        if p.get("type") == "text":
            return p.get("text") or ""
    return ""


def _generate_sync(models: Models, transcript: list[dict]) -> str:
    tokenizer, model = ensure_mini(models)
    try:
        text = tokenizer.apply_chat_template(
            transcript,
            tokenize=False,
            add_generation_prompt=True,
            enable_thinking=True,
        )
    except TypeError:
        text = tokenizer.apply_chat_template(
            transcript, tokenize=False, add_generation_prompt=True
        )
    inputs = tokenizer(text, return_tensors="pt").to(DEVICE)
    try:
        with torch.inference_mode():
            out = model.generate(
                **inputs,
                max_new_tokens=MAX_NEW_TOKENS,
                do_sample=False,
                pad_token_id=tokenizer.eos_token_id,
                use_cache=True,
            )
        gen_ids = out[0][inputs["input_ids"].shape[1] :]
        decoded = tokenizer.decode(gen_ids, skip_special_tokens=True).strip()
    finally:
        del inputs
        if DEVICE == "cuda":
            torch.cuda.empty_cache()
        gc.collect()
    return decoded


def _parse_json(raw: str) -> dict:
    s = _THINK_RE.sub("", raw).strip()
    s = _FENCE_RE.sub("", s).strip()
    try:
        return json.loads(s)
    except json.JSONDecodeError:
        start, end = s.find("{"), s.rfind("}")
        if start >= 0 and end > start:
            return json.loads(s[start : end + 1])
        raise


async def _run_tool(http: aiohttp.ClientSession, name: str, args: dict) -> Any:
    try:
        if name == "web_search":
            return await tools.web_search(http, args.get("query", ""))
        if name == "fetch_page":
            return await tools.fetch_page(http, args.get("url", ""))
        if name == "mb_search":
            return await tools.mb_search(http, args.get("query", ""))
        if name == "genius_search":
            return await tools.genius_search(http, args.get("query", ""))
        if name == "wikipedia_search":
            return await tools.wikipedia_search(http, args.get("query", ""))
        if name == "discogs_search":
            return await tools.discogs_search(http, args.get("query", ""))
        return {"error": f"unknown tool: {name}"}
    except Exception as e:
        log.warning(f"[resolve] tool {name} crashed: {e}")
        return {"error": f"{name} exception: {e}"}


def _trim_transcript(transcript: list[dict]) -> None:
    """Если разговор разрастается — выкидываем самые старые шаги, оставляя
    первый user-message (системную инструкцию) и хвост."""
    while len(transcript) > 3 and _transcript_size(transcript) > MAX_TRANSCRIPT_CHARS:
        del transcript[1:3]


def _transcript_size(transcript: list[dict]) -> int:
    return sum(len(m.get("content") or "") for m in transcript)


def _normalize(data: dict) -> dict:
    primary_raw = data.get("primary_artist")
    primary = primary_raw.strip() if isinstance(primary_raw, str) and primary_raw.strip() else None

    featured = _string_list(data.get("featured"))
    producers = _string_list(data.get("producers"))
    remixers = _string_list(data.get("remixers"))

    album = data.get("album")
    album_out = None
    if isinstance(album, dict):
        title_raw = album.get("title")
        if isinstance(title_raw, str) and title_raw.strip():
            year = album.get("year")
            if not isinstance(year, int):
                year = None
            pa = album.get("primary_artist")
            pa_str = pa.strip() if isinstance(pa, str) and pa.strip() else None
            album_out = {
                "title": title_raw.strip(),
                "year": year,
                "primary_artist": pa_str,
            }

    conf = data.get("confidence")
    try:
        conf_val = float(conf)
    except (TypeError, ValueError):
        conf_val = 0.0
    conf_val = max(0.0, min(1.0, conf_val))

    return {
        "primary_artist": primary,
        "featured": featured,
        "producers": producers,
        "remixers": remixers,
        "album": album_out,
        "confidence": conf_val,
    }


def _string_list(v: Any) -> list[str]:
    if not isinstance(v, list):
        return []
    out = []
    for item in v:
        if isinstance(item, str):
            s = item.strip()
            if s:
                out.append(s)
    return out


def _empty_answer() -> dict:
    return {
        "primary_artist": None,
        "featured": [],
        "producers": [],
        "remixers": [],
        "album": None,
        "confidence": 0.0,
    }


VERIFY_PROMPT = """Тебе дано предполагаемое название трека и его исполнитель.
Скажи существует ли такой трек у такого исполнителя в реальности.

Используй инструменты mb_search / genius_search / wikipedia_search / web_search чтобы проверить.
Если найден трек с этим точным сочетанием artist+title в базах MB или Genius — exists=true.
Если ничего похожего — exists=false.
Если есть похожие но не точно совпадающие — exists=null (неясно).

# Вход
artist: {artist}
title: {title}

# Протокол

На каждом шаге выводи РОВНО ОДИН JSON.

Запрос инструмента:
{{"action": "tool", "tool": "<имя>", "args": {{...}}}}

Финальный ответ:
{{"action": "answer", "data": {{"exists": true|false|null, "confidence": 0.0..1.0}}}}"""


MATCH_PROMPT = """Тебе дан целевой трек и список SoundCloud-кандидатов.
Выбери ИНДЕКС кандидата, который точно соответствует целевому (тот же артист и
то же название), с учётом мусора в SC-тайтле:
- "Artist - Title" префикс с любыми разделителями (-, —, –)
- (feat. ...), (prod. ...), [Free DL], (Original Mix), (Remix), и т.п.
- разная пунктуация и регистр

Если ни один не совпадает уверенно — верни match_id=null.

# Цель
artist: {target_artist}
title:  {target_title}

# Кандидаты
{candidates}

# Ответ
РОВНО один JSON, без пояснений:
{{"match_id": <int|null>, "confidence": 0.0..1.0}}"""


async def match_track(models: Models, payload: dict) -> dict:
    target = payload.get("target") or {}
    target_artist = (target.get("artist") or "").strip()[:200]
    target_title = (target.get("title") or "").strip()[:300]
    cands_raw = payload.get("candidates") or []
    if not target_title or not isinstance(cands_raw, list) or not cands_raw:
        return {"match_id": None, "confidence": 0.0}

    cands = []
    for c in cands_raw[:20]:
        if not isinstance(c, dict):
            continue
        cid = c.get("id")
        if not isinstance(cid, int):
            continue
        cands.append({
            "id": cid,
            "artist": (c.get("artist") or "")[:200],
            "title": (c.get("title") or "")[:300],
            "uploader": (c.get("uploader") or "")[:200],
            "duration_sec": c.get("duration_sec"),
        })
    if not cands:
        return {"match_id": None, "confidence": 0.0}

    cand_lines = []
    for c in cands:
        dur = c["duration_sec"]
        dur_s = f"{dur}s" if isinstance(dur, int) else "?"
        cand_lines.append(
            f"{c['id']}. artist={c['artist']!r} title={c['title']!r} "
            f"uploader={c['uploader']!r} duration={dur_s}"
        )
    initial = MATCH_PROMPT.format(
        target_artist=target_artist or "(unknown)",
        target_title=target_title,
        candidates="\n".join(cand_lines),
    )
    transcript = [{"role": "user", "content": initial}]
    valid_ids = {c["id"] for c in cands}

    try:
        raw_out = await _generate(models, transcript)
    except Exception as e:
        log.warning(f"[match] generate failed: {e}")
        return {"match_id": None, "confidence": 0.0}
    try:
        msg = _parse_json(raw_out)
    except Exception as e:
        log.warning(f"[match] parse failed: {e}; out={raw_out!r}")
        return {"match_id": None, "confidence": 0.0}

    mid = msg.get("match_id")
    if not isinstance(mid, int) or mid not in valid_ids:
        return {"match_id": None, "confidence": 0.0}
    try:
        conf = float(msg.get("confidence") or 0.7)
    except (TypeError, ValueError):
        conf = 0.7
    return {"match_id": mid, "confidence": max(0.0, min(1.0, conf))}


async def verify_existence(models: Models, payload: dict) -> dict:
    artist = (payload.get("artist") or "").strip()[:200]
    title = (payload.get("title") or "").strip()[:300]
    if not artist or not title:
        return {"exists": None, "confidence": 0.0}

    initial = VERIFY_PROMPT.format(artist=artist, title=title)
    transcript = [{"role": "user", "content": initial}]

    timeout = aiohttp.ClientTimeout(total=tools.TOOL_TIMEOUT + 4)
    async with aiohttp.ClientSession(timeout=timeout) as http:
        for step in range(6):
            try:
                raw_out = await _generate(models, transcript)
            except Exception as e:
                log.warning(f"[verify] generate failed: {e}")
                return {"exists": None, "confidence": 0.0}
            try:
                msg = _parse_json(raw_out)
            except Exception:
                return {"exists": None, "confidence": 0.0}

            if msg.get("action") == "answer":
                data = msg.get("data") or {}
                exists = data.get("exists")
                if exists not in (True, False, None):
                    exists = None
                try:
                    conf = float(data.get("confidence") or 0.0)
                except (TypeError, ValueError):
                    conf = 0.0
                return {"exists": exists, "confidence": max(0.0, min(1.0, conf))}

            if msg.get("action") == "tool":
                tool_name = msg.get("tool") or ""
                args = msg.get("args") or {}
                tool_result = await _run_tool(http, tool_name, args)
                transcript.append(
                    {"role": "assistant", "content": json.dumps(msg, ensure_ascii=False)}
                )
                transcript.append(
                    {
                        "role": "user",
                        "content": json.dumps(
                            {"tool_result": tool_result}, ensure_ascii=False
                        ),
                    }
                )
                _trim_transcript(transcript)
                continue
            return {"exists": None, "confidence": 0.0}
    return {"exists": None, "confidence": 0.0}
