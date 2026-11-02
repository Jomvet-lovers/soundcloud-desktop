/** Spaced dashes — the same 7 variants as the backend's `split_first_dash` */
export const TITLE_SEPARATORS = [' - ', ' — ', ' – ', ' -- ', ' ‒ ', ' − ', ' ─ '] as const;

/**
 * Name-list joiners — a mirror of the backend's `RE_SPLIT_NAMES`
 * (artist_names.rs): comma/semicolon always; ligatures (x, Cyrillic
 * х, crosses, +, vs, &, and, w/, feat, /) — only when space-delimited, so that
 * "AC/DC", "She&Him" or "Axwell" are not cut.
 */
const SPLITTER =
  /\s*[,;]\s*|\s+(?:x|х|×|✕|✖|⨯|\+|vs\.?|&|and|w\/|feat\.?|ft\.?|featuring|\/)\s+/giu;

export interface NamePart {
  text: string;
  /** Offsets into the source string — needed to glue pieces back into a full name */
  start: number;
  end: number;
}

/** Split a list string into names with their positions in the source */
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
