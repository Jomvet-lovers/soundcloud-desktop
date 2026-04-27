export function cleanTitle(s: string): string {
  return s
    .replace(/\(feat\.?[^)]*\)/gi, '')
    .replace(/\(ft\.?[^)]*\)/gi, '')
    .replace(/\[.*?\]/g, '')
    .replace(
      /\(.*?(remix|edit|version|mix|cover|live|acoustic|instrumental|original|prod).*?\)/gi,
      '',
    )
    .replace(/\s+(feat\.?|ft\.?|featuring|prod\.?)\b.*$/gi, '')
    .replace(/\s+/g, ' ')
    .trim();
}

export function stripBrackets(s: string): string {
  return s
    .replace(/\([^)]*\)/g, '')
    .replace(/\[[^\]]*\]/g, '')
    .replace(/\s+/g, ' ')
    .trim();
}

export function alphaOnly(s: string): string {
  return s
    .replace(/[^\p{L}\p{N}\s]/gu, '')
    .replace(/\s+/g, ' ')
    .trim();
}

/** Каноничная форма артиста/названия для точного сравнения. */
export function canonMeta(s: string): string {
  return alphaOnly(stripBrackets(cleanTitle(s ?? ''))).toLowerCase();
}

export function splitArtistTitle(raw: string): [string, string] | null {
  for (const sep of [' - ', ' – ', ' — ', ' // ']) {
    const idx = raw.indexOf(sep);
    if (idx > 0) {
      const artist = raw.slice(0, idx).trim();
      const title = raw.slice(idx + sep.length).trim();
      if (artist && title) return [artist, title];
    }
  }
  return null;
}

export interface LyricLine {
  time: number;
  text: string;
}

export function parseLRC(lrc: string): LyricLine[] {
  const lines: LyricLine[] = [];
  for (const raw of lrc.split('\n')) {
    const m = raw.match(/^\[(\d{2}):(\d{2})\.(\d{2,3})\]\s*(.*)/);
    if (!m) continue;
    const time = +m[1] * 60 + +m[2] + +m[3].padEnd(3, '0') / 1000;
    const text = m[4].trim();
    if (text) lines.push({ time, text });
  }
  return lines;
}

export function stripLrcTimestamps(lrc: string): string {
  return lrc
    .split('\n')
    .map((line) => line.replace(/\[\d{2}:\d{2}\.\d{2,3}\]/g, '').trim())
    .filter(Boolean)
    .join('\n');
}

/**
 * Возвращает лучший доступный текст лирики для побочных задач (детект языка,
 * embed в qdrant). Приоритет — plain (там полный человекочитаемый текст), но
 * если plain пуст/null, а есть synced — отдаём stripLrcTimestamps(synced).
 * Нужно, чтобы не пропускать треки где источник вернул только синхронный LRC
 * (или где plain после очистки оказался коротким/пустым).
 */
export function pickLyricsText(plain: string | null, synced: string | null): string | null {
  const p = (plain ?? '').trim();
  if (p.length > 0) return p;
  const s = (synced ?? '').trim();
  if (!s) return null;
  const stripped = stripLrcTimestamps(s).trim();
  return stripped.length > 0 ? stripped : null;
}

/**
 * Эвристический детект языка по доминирующему скрипту в тексте. Используется
 * как fallback когда AI-детект отвалился (timeout / null) — лучше иметь
 * корректный ru/en/ja/ko чем null, фильтр по language тогда не работает.
 *
 * Возвращает null если текста мало или ни один скрипт не доминирует.
 */
export function detectLanguageHeuristic(
  text: string,
): { language: string; confidence: number } | null {
  if (!text) return null;
  const sample = text.slice(0, 4000);
  const counts = new Map<string, number>();
  let total = 0;
  for (const ch of sample) {
    const cp = ch.codePointAt(0);
    if (cp === undefined) continue;
    let script: string | null = null;
    if (cp >= 0x0400 && cp <= 0x04ff) script = 'cyrillic';
    else if (cp >= 0x0370 && cp <= 0x03ff) script = 'greek';
    else if (cp >= 0x0590 && cp <= 0x05ff) script = 'hebrew';
    else if (cp >= 0x0600 && cp <= 0x06ff) script = 'arabic';
    else if (cp >= 0xac00 && cp <= 0xd7af) script = 'hangul';
    else if (cp >= 0x3040 && cp <= 0x309f) script = 'hiragana';
    else if (cp >= 0x30a0 && cp <= 0x30ff) script = 'katakana';
    else if (cp >= 0x4e00 && cp <= 0x9fff) script = 'cjk';
    else if ((cp >= 0x41 && cp <= 0x5a) || (cp >= 0x61 && cp <= 0x7a)) script = 'latin';
    else continue;
    counts.set(script, (counts.get(script) ?? 0) + 1);
    total++;
  }
  if (total < 20) return null;

  let bestScript = 'latin';
  let bestCount = 0;
  for (const [s, c] of counts) {
    if (c > bestCount) {
      bestScript = s;
      bestCount = c;
    }
  }
  const ratio = bestCount / total;
  if (ratio < 0.3) return null;

  let language: string;
  switch (bestScript) {
    case 'cyrillic':
      language = 'ru';
      break;
    case 'hangul':
      language = 'ko';
      break;
    case 'hiragana':
    case 'katakana':
      language = 'ja';
      break;
    case 'cjk': {
      const ja = (counts.get('hiragana') ?? 0) + (counts.get('katakana') ?? 0);
      language = ja > 0 ? 'ja' : 'zh';
      break;
    }
    case 'arabic':
      language = 'ar';
      break;
    case 'hebrew':
      language = 'he';
      break;
    case 'greek':
      language = 'el';
      break;
    default:
      language = 'en';
  }
  return { language, confidence: ratio };
}

/**
 * Детерминированная генерация поисковых запросов для Genius/LRCLIB/Musixmatch.
 * Без LLM. Покрывает: re-upload каналы (nightcore/vibes/boost) где реальный артист вшит
 * в title как "Artist - Title", и декораторы ("official video", "sped up", скобки, feat.).
 */
export function heuristicQueries(artist: string, title: string): string[] {
  const out = new Set<string>();
  const add = (s: string) => {
    const cleaned = alphaOnly(s).replace(/\s+/g, ' ').trim();
    if (cleaned.length >= 2) out.add(cleaned);
  };

  const cleanT = cleanTitle(title);
  const strippedT = stripBrackets(title);

  // 1. Исходные комбинации
  add(`${artist} ${title}`);
  add(`${artist} ${cleanT}`);
  add(`${artist} ${strippedT}`);

  // 2. Re-upload pattern: "RealArtist - RealTitle" зашит в title
  const split = splitArtistTitle(title);
  if (split) {
    const [realArtist, realTitle] = split;
    add(`${realArtist} ${realTitle}`);
    add(`${realArtist} ${cleanTitle(realTitle)}`);
    add(cleanTitle(realTitle));
  }

  // 3. Только очищенный title — на случай мусорного uploader-артиста
  add(cleanT);
  add(strippedT);

  return [...out].slice(0, 6);
}
