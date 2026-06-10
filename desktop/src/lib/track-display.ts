import {useMemo} from 'react';
import type {EnrichmentArtist, Track, TrackAvailability} from '../stores/player';

export interface ArtistDisplay {
  primary: string;
  uploader: string | null;
  isEnriched: boolean;
  verified: boolean;
  confidence: number | null;
  pending: boolean;
  uploadKind: string | null;
  availability: TrackAvailability;
}

/** Единый результат разбора трека для отображения: что писать в заголовок
 *  и кого — в строку авторов. Все компоненты (карточки, очередь, плеер,
 *  оффлайн) обязаны брать отсюда, своих разборов не заводить. */
export interface TrackDisplay {
  title: string;
  /** Компоненты строки авторов в порядке показа. */
  artistNames: string[];
  /** Готовая строка авторов: "МОКЕРИ, Psychosis". */
  artistLine: string;
  /** true — авторы взяты из разметки "Artist1, Artist2 - Title" в тайтле. */
  fromTitleSplit: boolean;
}

export type UploadKind = 'original' | 'demo' | 'alt' | 'reupload' | 'cover' | 'unknown';

const TITLE_SEPARATORS = [' - ', ' — ', ' – ', ' -- ', ' ‒ ', ' − ', ' ─ '] as const;
const ARTIST_SPLITTERS =
  /\s*(?:,|;|&|\sx\s|\sх\s|\s[×✕✖⨯+]\s|\svs\.?\s|\sand\s|\sfeat\.?\s|\sft\.?\s|\sfeaturing\s|\sw\/\s|\s\/\s)\s*/i;

/** Максимум имён в левой части, чтобы не принять предложение с запятыми
 *  за список авторов. */
const MAX_TITLE_ARTISTS = 6;

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

function looksLikeRoleTag(inner: string): boolean {
  const lower = inner.trim().toLowerCase();
  const parts = lower.split(/\s+/).filter(Boolean);
  if (parts.length === 0) return false;
  return ROLE_TAG_HEAD.has(parts[0]) || ROLE_TAG_TAIL.has(parts[parts.length - 1]);
}

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
};
const COMBINING_MARKS = /[̀-ͯ᪰-᫿᷀-᷿⃐-⃿︠-︯]/g;

/**
 * Канонический ключ сравнения имён — зеркало бэкендового
 * `enrich::normalize::normalize_name`: fold стилизованного юникода и
 * диакритики, lowercase, только буквы/цифры, `&` ≡ "and", без ведущего "the".
 * Сравнивать имена где-либо ещё другим способом — нельзя.
 */
export function foldName(s: string): string {
  let folded = '';
  for (const ch of s.normalize('NFKD').replace(COMBINING_MARKS, '')) {
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

/**
 * Хвостовая транскрипция в скобках: "МОКЕРИ (moxckery)" / "трек (kavabanga)"
 * → срезаем. Триггер: outer-строка имеет non-Latin codepoint (>U+02AF), inner
 * это чисто ASCII-латиница (+пробел/дефис/апостроф). Role-теги ("трек (cover)",
 * "трек (someone remix)") НЕ трогаем — их обрабатывает stripInlineTags +
 * enrich + UI badge. "Beyoncé (Sasha Fierce)" не срезается (обе стороны latin).
 */
export function stripTranslitParens(s: string): string {
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
 * Inline-теги в круглых/квадратных скобках которые относятся к роли участника
 * (prod./feat./ft./featuring/remix/rmx/edit/version/cover/instrumental) —
 * срезаем из отображаемого title'а. Информация о ролях уже есть в
 * `enrichment.participants` и показывается отдельным блоком на TrackPage,
 * чтобы заголовок не превращался в свалку.
 *
 * "Free DL"-носинг тоже срезаем: SC-аплоадеры обожают [Free DL]/(out now)/HQ.
 */
const TAG_PATTERN =
  /\s*[([][^)\]]*(?:prod\.?|produced\s+by|prod\s+by|feat\.?|featuring|ft\.?|with|remix|rmx|edit|version|cover|instrumental|free\s+(?:dl|download)|out\s+now|original\s+mix|extended\s+mix|radio\s+edit|premiere|exclusive|hd|hq|official(?:\s+(?:audio|video))?|lyrics|lyric\s+video|visualizer)\b[^)\]]*[)\]]/gi;

export function stripInlineTags(title: string): string {
  let prev = title;
  for (let i = 0; i < 4; i++) {
    const next = prev
      .replace(TAG_PATTERN, '')
      .replace(/\s{2,}/g, ' ')
      .trim();
    if (next === prev) break;
    prev = next;
  }
  return prev || title;
}

type DisplayInput = Pick<Track, 'title' | 'user' | 'enrichment'>;

/** Co-primary артисты из enrichment.participants (role='primary'). */
function coPrimaryNames(track: DisplayInput): string[] {
  return (
    track.enrichment?.participants
      ?.filter((p) => p.role === 'primary' && p.artist?.name)
      .map((p) => p.artist.name) ?? []
  );
}

function dedupeByFold(names: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const n of names) {
    const key = foldName(n);
    if (!key || seen.has(key)) continue;
    seen.add(key);
    out.push(n);
  }
  return out;
}

/**
 * Единый разбор трека для отображения.
 *
 * Если левая часть "… - …" содержит ХОТЯ БЫ одного известного артиста
 * (enrichment primary/co-primary/участники/uploader), вся левая часть — это
 * список авторов, правая — название: "МОКЕРИ, Psychosis - kill" при известном
 * «МОКЕРИ» даёт авторов "МОКЕРИ, Psychosis" и название "kill".
 * Иначе авторы берутся из enrichment (primary + co-primary) или uploader.
 */
export function getTrackDisplay(track: DisplayInput): TrackDisplay {
  const cleaned = stripTranslitParens(stripInlineTags(track.title));

  const known = new Set<string>();
  const addKnown = (name: string | undefined | null) => {
    if (!name) return;
    const key = foldName(name);
    if (key) known.add(key);
    // Комбинированное имя из enrichment ("МОКЕРИ, Psychosis") должно
    // матчить и своими компонентами.
    for (const part of name.split(ARTIST_SPLITTERS)) {
      const pk = foldName(part);
      if (pk) known.add(pk);
    }
  };
  const primaryName = track.enrichment?.primary_artist?.name;
  addKnown(primaryName);
  for (const n of coPrimaryNames(track)) addKnown(n);
  for (const p of track.enrichment?.participants ?? []) addKnown(p.artist?.name);
  const uploader = track.user?.username?.trim() ?? '';
  addKnown(uploader);

  const splitParts = (s: string) =>
    s
      .split(ARTIST_SPLITTERS)
      .map((p) => p.trim())
      .filter(Boolean);

  for (const sep of TITLE_SEPARATORS) {
    const idx = cleaned.indexOf(sep);
    if (idx <= 0) continue;
    const left = cleaned.slice(0, idx).trim();
    const right = cleaned.slice(idx + sep.length).trim();
    if (!right) continue;
    const leftParts = splitParts(left);
    if (leftParts.length === 0 || leftParts.length > MAX_TITLE_ARTISTS) continue;
    if (leftParts.some((p) => known.has(foldName(p)))) {
      const artistNames = dedupeByFold(leftParts);
      return {
        title: right,
        artistNames,
        artistLine: artistNames.join(', '),
        fromTitleSplit: true,
      };
    }
  }

  // Перевёрнутая разметка "Track - Artist" (~4% дефисных тайтлов): слева
  // ничего знакомого, справа — целиком известные артисты.
  for (const sep of TITLE_SEPARATORS) {
    const idx = cleaned.indexOf(sep);
    if (idx <= 0) continue;
    const left = cleaned.slice(0, idx).trim();
    const right = cleaned.slice(idx + sep.length).trim();
    if (!left || !right) continue;
    const rightParts = splitParts(right);
    if (rightParts.length === 0 || rightParts.length > MAX_TITLE_ARTISTS) continue;
    if (rightParts.every((p) => known.has(foldName(p)))) {
      const artistNames = dedupeByFold(rightParts);
      return {
        title: left,
        artistNames,
        artistLine: artistNames.join(', '),
        fromTitleSplit: true,
      };
    }
  }

  // Голый дефис ("Уннв-Без даты"): режем только когда левая часть целиком —
  // известный артист, иначе порвём имя вида "x-ray".
  const bareIdx = cleaned.indexOf('-');
  if (bareIdx > 0 && bareIdx + 1 < cleaned.length && cleaned[bareIdx + 1] !== '-') {
    const left = cleaned.slice(0, bareIdx).trim();
    const right = cleaned.slice(bareIdx + 1).trim();
    if (left && right && known.has(foldName(left))) {
      return {
        title: right,
        artistNames: [left],
        artistLine: left,
        fromTitleSplit: true,
      };
    }
  }

  const enriched = primaryName ? [primaryName, ...coPrimaryNames(track)] : [];
  const artistNames = dedupeByFold(enriched.length ? enriched : uploader ? [uploader] : []);
  return {
    title: cleaned,
    artistNames,
    artistLine: artistNames.join(', '),
    fromTitleSplit: false,
  };
}

export function getArtistDisplay(track: DisplayInput): ArtistDisplay {
  const enrichment = track.enrichment;
  const real = enrichment?.primary_artist;
  const uploader = track.user?.username ?? '';
  const availability = (enrichment?.availability ?? 'indexed') as TrackAvailability;
  const pending = enrichment?.state === 'pending' || (!enrichment && availability === 'indexed');
  const uploadKind =
    enrichment?.upload_kind && enrichment.upload_kind !== 'unknown' ? enrichment.upload_kind : null;

  const display = getTrackDisplay(track);
  const isEnriched = Boolean(real?.name?.trim());

  if (!display.artistLine) {
    return {
      primary: uploader,
      uploader: null,
      isEnriched: false,
      verified: false,
      confidence: null,
      pending,
      uploadKind,
      availability,
    };
  }

  const uploaderShownAsArtist = display.artistNames.some((n) => foldName(n) === foldName(uploader));
  return {
    primary: display.artistLine,
    uploader: uploaderShownAsArtist || !uploader || availability !== 'indexed' ? null : uploader,
    isEnriched,
    verified: real?.verified === true,
    confidence: real?.confidence ?? null,
    pending: isEnriched ? false : pending,
    uploadKind,
    availability,
  };
}

export function getDisplayTitle(track: DisplayInput): string {
  return getTrackDisplay(track).title;
}

export function useTrackDisplay(track: DisplayInput): TrackDisplay {
  // biome-ignore lint/correctness/useExhaustiveDependencies: разбор зависит от перечисленных скалярных срезов track
  return useMemo(
    () => getTrackDisplay(track),
    [
      track.title,
      track.user?.username,
      track.enrichment?.primary_artist?.name,
      track.enrichment?.participants,
    ],
  );
}

export function useArtistDisplay(track: DisplayInput): ArtistDisplay {
  // biome-ignore lint/correctness/useExhaustiveDependencies: разбор зависит от перечисленных скалярных срезов track
  return useMemo(
    () => getArtistDisplay(track),
    [
      track.title,
      track.user?.username,
      track.enrichment?.primary_artist?.name,
      track.enrichment?.primary_artist?.verified,
      track.enrichment?.primary_artist?.confidence,
      track.enrichment?.upload_kind,
      track.enrichment?.availability,
      track.enrichment?.state,
      track.enrichment?.participants,
    ],
  );
}

export function useDisplayTitle(track: DisplayInput): string {
  return useTrackDisplay(track).title;
}

/** Имя в строке авторов + куда оно ведёт (null — некликабельно). */
export interface ArtistLinkItem {
  name: string;
  target: string | null;
}

/**
 * Поимённые ссылки для строки авторов: каждое имя из `getTrackDisplay`
 * матчится (fold) на enrichment primary / co-primary / участников (→ страница
 * артиста) или на uploader'а (→ страница юзера). Несматченное — просто текст.
 */
export function getArtistLinkItems(track: DisplayInput): ArtistLinkItem[] {
  const display = getTrackDisplay(track);
  const targets = new Map<string, string>();
  const put = (name: string | undefined | null, target: string | null) => {
    if (!name || !target) return;
    const key = foldName(name);
    if (key && !targets.has(key)) targets.set(key, target);
  };
  const primary = track.enrichment?.primary_artist;
  if (primary?.id) put(primary.name, `/artist/${encodeURIComponent(primary.id)}`);
  for (const p of track.enrichment?.participants ?? []) {
    if (p.artist?.id) put(p.artist.name, `/artist/${encodeURIComponent(p.artist.id)}`);
  }
  if (track.user?.urn) put(track.user.username, `/user/${encodeURIComponent(track.user.urn)}`);
  return display.artistNames.map((name) => ({
    name,
    target: targets.get(foldName(name)) ?? null,
  }));
}

export function useArtistLinkItems(track: DisplayInput): ArtistLinkItem[] {
  // biome-ignore lint/correctness/useExhaustiveDependencies: разбор зависит от перечисленных скалярных срезов track
  return useMemo(
    () => getArtistLinkItems(track),
    [
      track.title,
      track.user?.username,
      track.user?.urn,
      track.enrichment?.primary_artist?.name,
      track.enrichment?.primary_artist?.id,
      track.enrichment?.participants,
    ],
  );
}

export function getArtistTarget(track: Pick<Track, 'user' | 'enrichment'>): string | null {
  const real = track.enrichment?.primary_artist;
  if (real?.id && real.verified) {
    return `/artist/${encodeURIComponent(real.id)}`;
  }
  if (track.user?.urn) {
    return `/user/${encodeURIComponent(track.user.urn)}`;
  }
  return null;
}

export interface ParticipantsBreakdown {
  featured: EnrichmentArtist[];
  remixers: EnrichmentArtist[];
  producers: EnrichmentArtist[];
}

export function getParticipants(
  track: Pick<Track, 'enrichment'>,
  roles: ReadonlyArray<string> = ['featured', 'remixer', 'producer'],
): ParticipantsBreakdown | null {
  const items = track.enrichment?.participants?.filter((p) => roles.includes(p.role)) ?? [];
  if (items.length === 0) return null;
  const featured = items.filter((p) => p.role === 'featured').map((p) => p.artist);
  const remixers = items.filter((p) => p.role === 'remixer').map((p) => p.artist);
  const producers = items.filter((p) => p.role === 'producer').map((p) => p.artist);
  if (featured.length === 0 && remixers.length === 0 && producers.length === 0) return null;
  return { featured, remixers, producers };
}
