/** Малые капители, которыми стилизуют ники (ᴍᴏɴᴀʀᴄʜ). NFKD их не трогает. */
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
  // Музыкальная стилизация: $ = s (A$AP, Ke$ha) — как на бэке.
  $: 's',
};

/** Вся категория Mark — как `is_combining_mark` на бэке (таблицы диапазонов
 *  пропускали кириллические U+0483-0489 и пр.). */
const COMBINING_MARKS = /\p{M}/gu;

/** Невидимые форматирующие символы (ZWSP, BOM, soft-hyphen, object
 *  replacement) — бэковый `is_invisible` их ВЫБРАСЫВАЕТ, не заменяет
 *  пробелом; иначе "МО​КЕРИ" фолдится в "мо кери" ≠ "мокери". */
const INVISIBLE = /[\u200B-\u200F\uFEFF\u2060\uFFFC\u00AD]/g;

/**
 * Канонический ключ сравнения имён — зеркало бэкендового
 * `enrich::normalize::normalize_name`: NFKD-fold стилизованного юникода и
 * диакритики, выброс невидимых символов, lowercase, только буквы/цифры,
 * `&` ≡ "and", `$` ≡ s, без ведущего "the".
 * Сравнивать имена где-либо ещё другим способом — нельзя.
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
      // апострофы схлопываем: Don't == Dont
    } else if (!prevSpace) {
      out += ' ';
      prevSpace = true;
    }
  }
  out = out.trim();
  return out.startsWith('the ') ? out.slice(4) : out;
}
