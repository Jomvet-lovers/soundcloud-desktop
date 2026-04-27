"""AI RPC: detect_language, search_queries, rank_lyrics, encode_text_mulan.

Транскрибация вынесена в transcribe.py — там тяжёлый pipeline с demucs + whisper.
"""
import asyncio
import gc
import json
import logging
import re

import torch

from ..models import DEVICE, Models, ensure_mini

log = logging.getLogger(__name__)


SEARCH_PROMPT = """Ты генерируешь запросы для поиска текстов песен (Genius, LRCLIB, Musixmatch).

Вход: артист + название трека с SoundCloud. Частые проблемы:
- Аплоадер — re-upload канал (nightcore/vibes/boost/chill/slowed), настоящий артист спрятан в title вида "Artist - Song".
- В title мусор: "official video/audio/lyrics", "sped up", "slowed", "remix", скобки [], (), "feat.", "prod.", эмодзи.
- Название может быть не на английском (русский, японский, корейский — сохраняй оригинальное написание).

Задача: верни 3 коротких поисковых запроса. Начинай с самого уверенного.

Правила:
1. Первый запрос — "реальный артист + реальное название" (без мусора).
2. Если в title есть "Artist - Song" — вытащи их как настоящие артист/название.
3. Убирай: скобки с remix/slowed/sped/nightcore/official/audio/video/lyrics, feat/ft/prod.
4. Сохраняй язык оригинала, не транслитерируй.
5. Только JSON, никаких пояснений.

Примеры:

artist="Nightcore Vibes", title="Billie Eilish - Ocean Eyes [Nightcore Remix]"
{{"queries": ["Billie Eilish Ocean Eyes", "Ocean Eyes", "Billie Eilish"]}}

artist="The Weeknd", title="Blinding Lights (Official Audio)"
{{"queries": ["The Weeknd Blinding Lights", "Blinding Lights", "Weeknd Blinding Lights"]}}

artist="Psychosis", title="рассвет"
{{"queries": ["Psychosis рассвет", "рассвет Psychosis", "рассвет"]}}

artist="Chill Beats", title="Post Malone ft. Swae Lee — Sunflower (Slowed + Reverb)"
{{"queries": ["Post Malone Swae Lee Sunflower", "Sunflower Post Malone", "Post Malone Sunflower"]}}

Теперь:
artist="{artist}", title="{title}"

Верни ТОЛЬКО JSON:
{{"queries": ["...", "...", "..."]}}"""


_THINK_RE = re.compile(r"<think>.*?</think>", re.DOTALL | re.IGNORECASE)


def _mini_generate(models: Models, prompt: str, max_new_tokens: int = 200) -> str:
    tokenizer, model = ensure_mini(models)
    try:
        text = tokenizer.apply_chat_template(
            [{"role": "user", "content": prompt}],
            tokenize=False,
            add_generation_prompt=True,
            enable_thinking=True,
        )
    except TypeError:
        # Токенизатор без enable_thinking — фолбэк.
        text = tokenizer.apply_chat_template(
            [{"role": "user", "content": prompt}],
            tokenize=False,
            add_generation_prompt=True,
        )
    inputs = tokenizer(text, return_tensors="pt").to(DEVICE)
    try:
        with torch.inference_mode():
            out = model.generate(
                **inputs,
                max_new_tokens=max_new_tokens,
                do_sample=False,
                pad_token_id=tokenizer.eos_token_id,
                use_cache=True,
            )
        gen_ids = out[0][inputs["input_ids"].shape[1]:]
        decoded = tokenizer.decode(gen_ids, skip_special_tokens=True).strip()
    finally:
        del inputs
        if DEVICE == "cuda":
            torch.cuda.empty_cache()
        gc.collect()
    return decoded


def _extract_json(raw: str) -> dict:
    s = _THINK_RE.sub("", raw).strip()
    # Снять ```json ... ``` обёртку.
    if s.startswith("```"):
        s = s.split("\n", 1)[1] if "\n" in s else s[3:]
        if s.endswith("```"):
            s = s[: -3]
    try:
        return json.loads(s)
    except json.JSONDecodeError:
        start = s.find("{")
        end = s.rfind("}")
        if start >= 0 and end > start:
            return json.loads(s[start : end + 1])
        raise


def _coerce_queries(raw_list) -> list[str]:
    """Принимает список строк или объектов с ключом query/q/text/search/string."""
    out: list[str] = []
    if not isinstance(raw_list, list):
        return out
    for item in raw_list:
        if isinstance(item, str):
            q = item.strip()
            if q:
                out.append(q)
        elif isinstance(item, dict):
            for key in ("query", "q", "text", "search", "string"):
                val = item.get(key)
                if isinstance(val, str) and val.strip():
                    out.append(val.strip())
                    break
    # dedupe сохраняя порядок
    seen: set[str] = set()
    uniq: list[str] = []
    for q in out:
        if q not in seen:
            seen.add(q)
            uniq.append(q)
    return uniq


async def encode_text_mulan(models: Models, payload: dict) -> dict:
    """MuQ-MuLan text tower → 512-dim вектор для поиска аудио по описанию."""
    text = (payload.get("text") or "").strip()
    if not text:
        raise ValueError("text is empty")

    def _run() -> dict:
        with torch.no_grad():
            vec = models.mulan(texts=[text[:512]]).squeeze()
        vec = vec / vec.norm()
        return {"vector": vec.detach().float().cpu().numpy().tolist()}

    async with models.mulan_lock:
        return await asyncio.to_thread(_run)


async def detect_language(models: Models, payload: dict) -> dict:
    text = (payload.get("text") or "").strip()
    if not text:
        raise ValueError("text is empty")

    def _run() -> dict:
        inputs = models.lang_tokenizer(
            text[:2000], return_tensors="pt", truncation=True, max_length=512
        )
        inputs = {k: v.to(DEVICE) for k, v in inputs.items()}
        with torch.no_grad():
            logits = models.lang_model(**inputs).logits
        probs = torch.softmax(logits, dim=-1).squeeze()
        best_idx = int(torch.argmax(probs).item())
        return {
            "language": models.lang_id2label[best_idx],
            "confidence": float(probs[best_idx].item()),
        }

    return await asyncio.to_thread(_run)


async def search_queries(models: Models, payload: dict) -> dict:
    artist = (payload.get("artist") or "")[:200]
    title = (payload.get("title") or "")[:300]
    prompt = SEARCH_PROMPT.format(artist=artist, title=title)
    log.info(f"[ai] search_queries input artist='{artist}' title='{title}'")

    def _run() -> dict:
        raw = ""
        try:
            raw = _mini_generate(models, prompt, max_new_tokens=3000)
            log.info(f"[ai] search_queries raw LLM output: {raw!r}")
            data = _extract_json(raw)
            queries = _coerce_queries(data.get("queries", []))
            if not queries:
                log.warning(
                    "[ai] search_queries: no valid queries coerced from LLM output, fallback"
                )
                queries = [f"{artist} {title}".strip()]
            log.info(f"[ai] search_queries coerced: {queries}")
            return {"queries": queries[:4]}
        except Exception as e:
            log.warning(f"[ai] search_queries: parse/gen failed ({e}), raw={raw!r}")
            return {"queries": [f"{artist} {title}".strip()], "fallback": str(e)}

    async with models.mini_lock:
        return await asyncio.to_thread(_run)


async def rank_lyrics(models: Models, payload: dict) -> dict:
    """Ранжирование через bge-m3 cosine: embed(target) vs embed(candidate.snippet), argmax."""
    candidates = payload.get("candidates") or []
    if not candidates:
        raise ValueError("candidates is empty")
    artist = (payload.get("artist") or "")[:200]
    title = (payload.get("title") or "")[:300]
    target = f"{artist} - {title}".strip(" -")

    def _run() -> dict:
        try:
            snippets = [(c.get("snippet") or "")[:500] for c in candidates]
            embs = models.lyrics_embed.encode(
                [target] + snippets, normalize_embeddings=True, convert_to_numpy=True
            )
            target_vec = embs[0]
            cand_vecs = embs[1:]
            sims = cand_vecs @ target_vec  # cosine, уже нормированы
            scores = [
                {"idx": c.get("idx", i), "score": float(round(sims[i] * 10, 3))}
                for i, c in enumerate(candidates)
            ]
            best_i = int(sims.argmax())
            return {
                "best_idx": int(candidates[best_i].get("idx", best_i)),
                "score": float(round(sims[best_i] * 10, 3)),
                "scores": scores,
            }
        except Exception as e:
            log.warning(f"[ai] rank_lyrics failed: {e}")
            return {"best_idx": candidates[0].get("idx", 0), "score": 0, "error": str(e)}

    return await asyncio.to_thread(_run)
