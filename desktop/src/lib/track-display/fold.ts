/** Small caps used to style nicks (ᴍᴏɴᴀʀᴄʜ). NFKD leaves them alone */
const SMALL_CAPS: Record<string, string> = {
  ᴀ: 'a',
  ʙ: 'b',
  ᴄ: 'c',
  ᴅ: 'd',
  ᴇ: 'e',
  ꜰ: 'f',
  ɢ: 'g',
  ʜ: 'h',
  ɪ: 'i',
  ᴊ: 'j',
  ᴋ: 'k',
  ʟ: 'l',
  ᴍ: 'm',
  ɴ: 'n',
  ᴏ: 'o',
  ᴘ: 'p',
  ꞯ: 'q',
  ʀ: 'r',
  ꜱ: 's',
  ᴛ: 't',
  ᴜ: 'u',
  ᴠ: 'v',
  ᴡ: 'w',
  ʏ: 'y',
  ᴢ: 'z',
  // Musical styling: $ = s (A$AP, Ke$ha) — same as the backend.
  $: 's',
};

/** The whole Mark category — like `is_combining_mark` on the backend (range
 *  tables used to miss Cyrillic U+0483-0489 and friends). */
const COMBINING_MARKS = /\p{M}/gu;

/** Invisible formatting characters (ZWSP, BOM, soft-hyphen, object
 *  replacement) — backend `is_invisible` DROPS them, it does not replace
 *  them with a space; otherwise "МО​КЕРИ" folds to "мо кери" ≠ "мокери". */
const INVISIBLE = /[\u200B-\u200F\uFEFF\u2060\uFFFC\u00AD]/g;

/**
 * Canonical key for comparing names — a mirror of the backend's
 * `enrich::normalize::normalize_name`: NFKD-fold of styled unicode and
 * diacritics, invisible characters dropped, lowercase, letters/digits only,
 * `&` ≡ "and", `$` ≡ s, no leading "the".
 * Comparing names anywhere else by another method is not allowed.
 */
export function foldName(s: string): string {
  let folded = '';
  for (const ch of s.normalize('NFKD').replace(COMBINING_MARKS, '').replace(INVISIBLE, '')) {
    folded += SMALL_CAPS[ch] ?? ch;
  }
  folded = folded.toLowerCase();
  let out = '';
  let prevSpace = true;
  for (const ch of folded) {
    if (ch === '&') {
      if (!prevSpace) out += ' ';
      out += 'and ';
      prevSpace = true;
    } else if (/[\p{L}\p{N}]/u.test(ch)) {
      out += ch;
      prevSpace = false;
    } else if (ch === "'" || ch === '’' || ch === 'ʼ' || ch === '`') {
      // collapse apostrophes: Don't == Dont
    } else if (!prevSpace) {
      out += ' ';
      prevSpace = true;
    }
  }
  out = out.trim();
  return out.startsWith('the ') ? out.slice(4) : out;
}
