/** Спейс-дефисы — те же 7 вариантов, что у бэкендового `split_first_dash`. */
export const TITLE_SEPARATORS = [' - ', ' — ', ' – ', ' -- ', ' ‒ ', ' − ', ' ─ '] as const;

/**
 * Сочленители списка имён — зеркало бэкендового `RE_SPLIT_NAMES`
 * (artist_names.rs): запятая/точка с запятой всегда; связки (x, кириллическая
 * х, кресты, +, vs, &, and, w/, feat, /) — только отбитые пробелами, чтобы не
 * порезать "AC/DC", "She&Him" или "Axwell".
 */
const SPLITTER =
  /\s*[,;]\s*|\s+(?:x|х|×|✕|✖|⨯|\+|vs\.?|&|and|w\/|feat\.?|ft\.?|featuring|\/)\s+/giu;

export interface NamePart {
  text: string;
  /** Смещения в исходной строке — нужны для склейки обратно в полное имя. */
  start: number;
  end: number;
}

/** Разбить строку-список на имена с позициями в исходнике. */
export function splitNamesWithOffsets(s: string): NamePart[] {
  const parts: NamePart[] = [];
  let cursor = 0;
  SPLITTER.lastIndex = 0;
  for (const m of s.matchAll(SPLITTER)) {
    const idx = m.index ?? 0;
    pushTrimmed(parts, s, cursor, idx);
    cursor = idx + m[0].length;
  }
  pushTrimmed(parts, s, cursor, s.length);
  return parts;
}

export function splitNames(s: string): string[] {
  return splitNamesWithOffsets(s).map((p) => p.text);
}

function pushTrimmed(parts: NamePart[], s: string, from: number, to: number) {
  let start = from;
  let end = to;
  while (start < end && /\s/.test(s[start])) start++;
  while (end > start && /\s/.test(s[end - 1])) end--;
  if (end > start) parts.push({ text: s.slice(start, end), start, end });
}
