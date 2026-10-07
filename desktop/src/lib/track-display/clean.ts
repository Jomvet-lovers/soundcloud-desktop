const ROLE_TAG_HEAD = new Set([
  'cover',
  'covers',
  'remix',
  'rmx',
  'edit',
  'version',
  'mix',
  'feat',
  'feat.',
  'ft',
  'ft.',
  'featuring',
  'prod',
  'prod.',
  'produced',
  'with',
  'vs',
  'vs.',
  'instrumental',
  'acoustic',
  'live',
  'demo',
  'bootleg',
  'flip',
  'mashup',
  'original',
  'extended',
  'radio',
  'free',
  'official',
  'premiere',
  'exclusive',
  'lyrics',
  'lyric',
  'visualizer',
  'hq',
  'hd',
]);
const ROLE_TAG_TAIL = new Set([
  'remix',
  'rmx',
  'edit',
  'mix',
  'version',
  'cover',
  'bootleg',
  'flip',
  'mashup',
  'instrumental',
  'acoustic',
]);

export function looksLikeRoleTag(inner: string): boolean {
  const lower = inner.trim().toLowerCase();
  const parts = lower.split(/\s+/).filter(Boolean);
  if (parts.length === 0) return false;
  return ROLE_TAG_HEAD.has(parts[0]) || ROLE_TAG_TAIL.has(parts[parts.length - 1]);
}

/**
 * Хвостовая транскрипция в скобках: "МОКЕРИ (moxckery)" → срезаем. Триггер:
 * outer имеет non-Latin codepoint (>U+02AF), inner — чисто ASCII-латиница.
 * Role-теги ("трек (cover)") не трогаем — их обрабатывает stripInlineTags.
 * "Beyoncé (Sasha Fierce)" не срезается (обе стороны latin).
 */
export function stripTranslitParens(s: string | null | undefined): string {
  if (!s) return '';
  const trimmed = s.replace(/\s+$/, '');
  if (!trimmed.endsWith(')')) return s;
  const open = trimmed.lastIndexOf('(');
  if (open <= 0) return s;
  const outer = trimmed.slice(0, open).replace(/\s+$/, '');
  const inner = trimmed.slice(open + 1, -1);
  if (!outer || !inner) return s;
  if (looksLikeRoleTag(inner)) return s;
  let outerHasNonLatin = false;
  for (const ch of outer) {
    const cp = ch.codePointAt(0) ?? 0;
    if (cp > 0x02af && /\p{L}/u.test(ch)) {
      outerHasNonLatin = true;
      break;
    }
  }
  const innerLatinOnly = /^[A-Za-z\s\-'`.]+$/.test(inner) && /[A-Za-z]/.test(inner);
  return outerHasNonLatin && innerLatinOnly ? outer : s;
}

/**
 * Inline-теги в скобках, относящиеся к роли участника (prod./feat./remix/…)
 * и [Free DL]-шум — срезаем из отображаемого title'а.
 *
 * Режем ТОЛЬКО когда внутри скобок не осталось имени: "(Remix)", "(Cover)",
 * "[Free DL]" уходят, а "(ETIA. Remix)" и "(prod. Иван)" остаются — иначе
 * приложение показывает «S0S» вместо официального «S0S (ETIA. Remix)».
 */
const TAG_PATTERN =
  /\s*[([][^)\]]*(?:prod\.?|produced\s+by|prod\s+by|feat\.?|featuring|ft\.?|with|remix|rmx|edit|version|cover|instrumental|free\s+(?:dl|download)|out\s+now|original\s+mix|extended\s+mix|radio\s+edit|premiere|exclusive|hd|hq|official(?:\s+(?:audio|video))?|lyrics|lyric\s+video|visualizer)\b[^)\]]*[)\]]/gi;

/** Слова, из которых целиком состоит role-скобка (шум, который режем). */
const NOISE_WORDS = [
  'prod',
  'produced',
  'prod.',
  'by',
  'feat',
  'feat.',
  'ft',
  'ft.',
  'featuring',
  'with',
  'remix',
  'rmx',
  'edit',
  'version',
  'mix',
  'cover',
  'instrumental',
  'free',
  'dl',
  'download',
  'out',
  'now',
  'original',
  'extended',
  'radio',
  'premiere',
  'exclusive',
  'hd',
  'hq',
  'official',
  'audio',
  'video',
  'lyrics',
  'lyric',
  'visualizer',
  'acoustic',
  'live',
  'demo',
  'bootleg',
  'flip',
  'mashup',
  'rework',
  'vip',
];

/**
 * В скобках осталось хоть что-то, кроме role-шума? Тогда это имя
 * («ETIA. Remix») и скобки трогать нельзя.
 */
function hasNameInside(inner: string): boolean {
  const words = inner
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s'.]/gu, ' ')
    .split(/\s+/)
    .filter(Boolean);
  return words.some((w) => !NOISE_WORDS.includes(w));
}

export function stripInlineTags(title: string | null | undefined): string {
  let prev = title ?? '';
  for (let i = 0; i < 4; i++) {
    const next = prev
      .replace(TAG_PATTERN, (match) => {
        const inner = match.replace(/^[\s([]+/, '').replace(/[)\]]+$/, '');
        return hasNameInside(inner) ? match : '';
      })
      .replace(/\s{2,}/g, ' ')
      .trim();
    if (next === prev) break;
    prev = next;
  }
  return prev || (title ?? '');
}
