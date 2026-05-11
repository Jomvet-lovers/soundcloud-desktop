"""Tools для resolve_artist: web search + fetch + MB/Genius lookup.

Каждая функция возвращает либо list/dict/str с полезной нагрузкой, либо
{"error": "..."} при провале. Никогда не бросает наружу — модель должна
видеть результат и сама решать что делать дальше.
"""
import asyncio
import logging
import os
import re
from urllib.parse import quote_plus, unquote, urlparse

import aiohttp

log = logging.getLogger(__name__)

TOOL_TIMEOUT = 12
UA_BROWSER = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
UA_MB = "scd-worker/0.1 ( https://scdinternal.site )"
MB_RATE_LIMIT_SEC = 1.1
MAX_PAGE_CHARS = 3000

_RESULT_RE = re.compile(
    r'<a[^>]*class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>'
    r'(?:.*?<a[^>]*class="result__snippet"[^>]*>(.*?)</a>)?',
    re.DOTALL,
)
_TAG_RE = re.compile(r"<[^>]+>")
_SCRIPT_RE = re.compile(r"<(script|style|noscript)\b.*?</\1>", re.DOTALL | re.IGNORECASE)
_WS_RE = re.compile(r"\s+")


async def web_search(http: aiohttp.ClientSession, query: str, limit: int = 6):
    q = (query or "").strip()
    if not q:
        return {"error": "empty query"}
    try:
        async with http.post(
            "https://html.duckduckgo.com/html/",
            data={"q": q},
            headers={"User-Agent": UA_BROWSER, "Accept": "text/html"},
            timeout=aiohttp.ClientTimeout(total=TOOL_TIMEOUT),
        ) as r:
            if r.status >= 400:
                return {"error": f"ddg status {r.status}"}
            html = await r.text()
    except asyncio.TimeoutError:
        return {"error": "ddg timeout"}
    except aiohttp.ClientError as e:
        return {"error": f"ddg: {e}"}

    items = []
    for m in _RESULT_RE.finditer(html):
        href = _unwrap_ddg(m.group(1))
        title = _strip_html(m.group(2) or "")
        snippet = _strip_html(m.group(3) or "")
        if not href or not title:
            continue
        items.append({"url": href, "title": title, "snippet": snippet})
        if len(items) >= limit:
            break
    return items


async def fetch_page(http: aiohttp.ClientSession, url: str):
    u = (url or "").strip()
    if not u or not u.startswith(("http://", "https://")):
        return {"error": "invalid url"}
    try:
        host = urlparse(u).hostname or ""
    except Exception:
        return {"error": "invalid url"}
    if not host:
        return {"error": "invalid host"}
    try:
        async with http.get(
            u,
            headers={"User-Agent": UA_BROWSER, "Accept": "text/html,*/*"},
            timeout=aiohttp.ClientTimeout(total=TOOL_TIMEOUT),
            allow_redirects=True,
        ) as r:
            if r.status >= 400:
                return {"error": f"http {r.status}"}
            ctype = (r.headers.get("Content-Type") or "").lower()
            if "html" not in ctype and "text" not in ctype:
                return {"error": f"unsupported content-type: {ctype}"}
            text = await r.text(errors="ignore")
    except asyncio.TimeoutError:
        return {"error": "fetch timeout"}
    except aiohttp.ClientError as e:
        return {"error": f"fetch: {e}"}

    cleaned = _SCRIPT_RE.sub(" ", text)
    cleaned = _TAG_RE.sub(" ", cleaned)
    cleaned = _WS_RE.sub(" ", cleaned).strip()
    return cleaned[:MAX_PAGE_CHARS]


_mb_lock = asyncio.Lock()
_mb_last_call = 0.0


async def mb_search(http: aiohttp.ClientSession, query: str):
    q = (query or "").strip()
    if not q:
        return {"error": "empty query"}
    await _mb_throttle()
    url = f"https://musicbrainz.org/ws/2/recording/?query={quote_plus(q)}&fmt=json&limit=5"
    try:
        async with http.get(
            url,
            headers={"User-Agent": UA_MB, "Accept": "application/json"},
            timeout=aiohttp.ClientTimeout(total=TOOL_TIMEOUT),
        ) as r:
            if r.status >= 400:
                return {"error": f"mb status {r.status}"}
            data = await r.json()
    except asyncio.TimeoutError:
        return {"error": "mb timeout"}
    except aiohttp.ClientError as e:
        return {"error": f"mb: {e}"}

    out = []
    for rec in (data.get("recordings") or [])[:5]:
        credits = rec.get("artist-credit") or []
        primary = None
        if credits:
            artist = credits[0].get("artist") or {}
            primary = credits[0].get("name") or artist.get("name")
        rels = rec.get("releases") or []
        rel = rels[0] if rels else {}
        date = rel.get("date") or ""
        out.append({
            "title": rec.get("title"),
            "artist": primary,
            "album": rel.get("title"),
            "year": (date[:4] or None) if date else None,
            "score": rec.get("score"),
        })
    return out


async def genius_search(http: aiohttp.ClientSession, query: str):
    q = (query or "").strip()
    if not q:
        return {"error": "empty query"}
    url = f"https://genius.com/api/search/multi?q={quote_plus(q)}"
    try:
        async with http.get(
            url,
            headers={"User-Agent": UA_BROWSER, "Accept": "application/json"},
            timeout=aiohttp.ClientTimeout(total=TOOL_TIMEOUT),
        ) as r:
            if r.status >= 400:
                return {"error": f"genius status {r.status}"}
            data = await r.json()
    except asyncio.TimeoutError:
        return {"error": "genius timeout"}
    except aiohttp.ClientError as e:
        return {"error": f"genius: {e}"}

    out = []
    sections = ((data.get("response") or {}).get("sections")) or []
    for sec in sections:
        if sec.get("type") != "song":
            continue
        for hit in (sec.get("hits") or [])[:5]:
            res = hit.get("result") or {}
            pa = (res.get("primary_artist") or {}).get("name")
            featured = [
                a.get("name")
                for a in (res.get("featured_artists") or [])
                if a.get("name")
            ]
            out.append({
                "title": res.get("title"),
                "primary_artist": pa,
                "featured": featured,
            })
    return out


async def wikipedia_search(http: aiohttp.ClientSession, query: str, limit: int = 5):
    q = (query or "").strip()
    if not q:
        return {"error": "empty query"}
    url = (
        "https://en.wikipedia.org/w/api.php"
        f"?action=query&format=json&list=search&utf8=1&srlimit={int(limit)}"
        f"&srsearch={quote_plus(q)}"
    )
    try:
        async with http.get(
            url,
            headers={"User-Agent": UA_BROWSER, "Accept": "application/json"},
            timeout=aiohttp.ClientTimeout(total=TOOL_TIMEOUT),
        ) as r:
            if r.status >= 400:
                return {"error": f"wiki status {r.status}"}
            data = await r.json()
    except asyncio.TimeoutError:
        return {"error": "wiki timeout"}
    except aiohttp.ClientError as e:
        return {"error": f"wiki: {e}"}

    out = []
    for item in ((data.get("query") or {}).get("search") or [])[:limit]:
        title = item.get("title")
        snippet = _strip_html(item.get("snippet") or "")
        if not title:
            continue
        out.append({
            "title": title,
            "snippet": snippet,
            "url": f"https://en.wikipedia.org/wiki/{quote_plus(title.replace(' ', '_'))}",
        })
    return out


async def discogs_search(http: aiohttp.ClientSession, query: str, limit: int = 5):
    q = (query or "").strip()
    if not q:
        return {"error": "empty query"}
    token = os.environ.get("DISCOGS_TOKEN", "").strip()
    if not token:
        return {"error": "DISCOGS_TOKEN not configured"}
    url = (
        "https://api.discogs.com/database/search"
        f"?q={quote_plus(q)}&type=release&per_page={int(limit)}"
    )
    try:
        async with http.get(
            url,
            headers={
                "User-Agent": "scd-worker/0.1",
                "Accept": "application/json",
                "Authorization": f"Discogs token={token}",
            },
            timeout=aiohttp.ClientTimeout(total=TOOL_TIMEOUT),
        ) as r:
            if r.status >= 400:
                return {"error": f"discogs status {r.status}"}
            data = await r.json()
    except asyncio.TimeoutError:
        return {"error": "discogs timeout"}
    except aiohttp.ClientError as e:
        return {"error": f"discogs: {e}"}

    out = []
    for item in (data.get("results") or [])[:limit]:
        title_raw = item.get("title") or ""
        artist = None
        track = None
        if " - " in title_raw:
            parts = title_raw.split(" - ", 1)
            artist = parts[0].strip()
            track = parts[1].strip()
        out.append({
            "title": track or title_raw,
            "artist": artist,
            "year": item.get("year"),
            "label": (item.get("label") or [None])[0] if isinstance(item.get("label"), list) else item.get("label"),
            "format": item.get("format"),
        })
    return out


async def _mb_throttle() -> None:
    global _mb_last_call
    async with _mb_lock:
        now = asyncio.get_event_loop().time()
        elapsed = now - _mb_last_call
        if elapsed < MB_RATE_LIMIT_SEC:
            await asyncio.sleep(MB_RATE_LIMIT_SEC - elapsed)
        _mb_last_call = asyncio.get_event_loop().time()


def _unwrap_ddg(href: str) -> str:
    if not href:
        return ""
    if href.startswith("//"):
        href = "https:" + href
    m = re.search(r"[?&]uddg=([^&]+)", href)
    if m:
        return unquote(m.group(1))
    return href


def _strip_html(s: str) -> str:
    s = _SCRIPT_RE.sub(" ", s)
    s = _TAG_RE.sub(" ", s)
    s = _WS_RE.sub(" ", s).strip()
    return s
