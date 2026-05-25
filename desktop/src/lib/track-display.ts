import { useMemo } from 'react';
import type { EnrichmentArtist, Track, TrackAvailability } from '../stores/player';

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

export type UploadKind = 'original' | 'demo' | 'alt' | 'reupload' | 'cover' | 'unknown';

const TITLE_SEPARATORS = [' - ', ' — ', ' – ', ' -- '] as const;
const ARTIST_SPLITTERS =
  /\s*(?:,|&|\sx\s|\s×\s|\svs\.?\s|\sand\s|\sfeat\.?\s|\sft\.?\s|\sfeaturing\s)\s*/i;

const ROLE_TAG_HEAD = new Set([
  'cover', 'covers', 'remix', 'rmx', 'edit', 'version', 'mix',
  'feat', 'feat.', 'ft', 'ft.', 'featuring',
  'prod', 'prod.', 'produced', 'with', 'vs', 'vs.',
  'instrumental', 'acoustic', 'live', 'demo', 'bootleg', 'flip', 'mashup',
  'original', 'extended', 'radio', 'free', 'official', 'premiere', 'exclusive',
  'lyrics', 'lyric', 'visualizer', 'hq', 'hd',
]);
const ROLE_TAG_TAIL = new Set([
  'remix', 'rmx', 'edit', 'mix', 'version', 'cover', 'bootleg',
  'flip', 'mashup', 'instrumental', 'acoustic',
]);

function looksLikeRoleTag(inner: string): boolean {
  const lower = inner.trim().toLowerCase();
  const parts = lower.split(/\s+/).filter(Boolean);
  if (parts.length === 0) return false;
  return ROLE_TAG_HEAD.has(parts[0]) || ROLE_TAG_TAIL.has(parts[parts.length - 1]);
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

export function getArtistDisplay(track: Pick<Track, 'user' | 'enrichment'>): ArtistDisplay {
  const enrichment = track.enrichment;
  const real = enrichment?.primary_artist;
  const uploader = track.user?.username ?? '';
  const availability = (enrichment?.availability ?? 'indexed') as TrackAvailability;
  const pending = enrichment?.state === 'pending' || (!enrichment && availability === 'indexed');
  const uploadKind =
    enrichment && enrichment.upload_kind && enrichment.upload_kind !== 'unknown'
      ? enrichment.upload_kind
      : null;
  if (!real || !real.name) {
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
  const realName = real.name.trim();
  if (!realName) {
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
  const sameAsUploader = realName.toLowerCase() === uploader.trim().toLowerCase();
  return {
    primary: realName,
    uploader: sameAsUploader || availability !== 'indexed' ? null : uploader || null,
    isEnriched: true,
    verified: real.verified === true,
    confidence: real.confidence ?? null,
    pending: false,
    uploadKind,
    availability,
  };
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
  /\s*[\(\[][^\)\]]*(?:prod\.?|produced\s+by|prod\s+by|feat\.?|featuring|ft\.?|with|remix|rmx|edit|version|cover|instrumental|free\s+(?:dl|download)|out\s+now|original\s+mix|extended\s+mix|radio\s+edit|premiere|exclusive|hd|hq|official(?:\s+(?:audio|video))?|lyrics|lyric\s+video|visualizer)\b[^\)\]]*[\)\]]/gi;

export function stripInlineTags(title: string): string {
  let prev = title;
  for (let i = 0; i < 4; i++) {
    const next = prev.replace(TAG_PATTERN, '').replace(/\s{2,}/g, ' ').trim();
    if (next === prev) break;
    prev = next;
  }
  return prev || title;
}

export function getDisplayTitle(track: Pick<Track, 'title' | 'user' | 'enrichment'>): string {
  let title = stripTranslitParens(stripInlineTags(track.title));

  const candidates = new Set<string>();
  const primary = track.enrichment?.primary_artist?.name;
  if (primary) candidates.add(primary.trim().toLowerCase());
  const uploader = track.user?.username;
  if (uploader) candidates.add(uploader.trim().toLowerCase());
  track.enrichment?.participants?.forEach((p) => {
    if (p.artist?.name) candidates.add(p.artist.name.trim().toLowerCase());
  });

  for (const sep of TITLE_SEPARATORS) {
    const idx = title.indexOf(sep);
    if (idx <= 0) continue;
    const left = title.slice(0, idx).trim();
    const right = title.slice(idx + sep.length).trim();
    if (!right) continue;
    const leftNames = left
      .split(ARTIST_SPLITTERS)
      .map((s) => s.trim().toLowerCase())
      .filter(Boolean);
    if (leftNames.length === 0) continue;
    if (leftNames.every((n) => candidates.has(n))) {
      return right;
    }
  }
  return title;
}

export function useArtistDisplay(track: Pick<Track, 'user' | 'enrichment'>): ArtistDisplay {
  return useMemo(
    () => getArtistDisplay(track),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      track.user?.username,
      track.enrichment?.primary_artist?.name,
      track.enrichment?.primary_artist?.verified,
      track.enrichment?.upload_kind,
      track.enrichment?.availability,
      track.enrichment?.state,
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

export function useDisplayTitle(track: Pick<Track, 'title' | 'user' | 'enrichment'>): string {
  return useMemo(
    () => getDisplayTitle(track),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      track.title,
      track.user?.username,
      track.enrichment?.primary_artist?.name,
      track.enrichment?.primary_artist?.verified,
      track.enrichment?.album?.title,
      track.enrichment?.participants?.length,
    ],
  );
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
