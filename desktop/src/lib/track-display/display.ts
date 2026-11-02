import type { Track, TrackAvailability } from '../../stores/player';
import { stripInlineTags, stripTranslitParens } from './clean';
import { foldName } from './fold';
import { type NamePart, splitNamesWithOffsets, TITLE_SEPARATORS } from './split';

export type DisplayInput = Pick<Track, 'title' | 'user' | 'enrichment'>;

/** The single parse result for a track's display: what goes into the title
 *  and who goes into the artist line. Every component (cards, queue, player,
 *  offline) must take it from here, not build its own parse. */
export interface TrackDisplay {
  title: string;
  /** The artist-line components in display order */
  artistNames: string[];
  /** The ready-made artist line: "МОКЕРИ, Psychosis" */
  artistLine: string;
  /** true — the artists came from "Artist1, Artist2 - Title" markup in the title */
  fromTitleSplit: boolean;
}

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

/** Max names on the left side, so a sentence with commas is not taken
 *  for an artist list. */
const MAX_TITLE_ARTISTS = 6;

/** Known track names by source strength:
 *  - `strong` — from enrichment (primary/co-primary/participants), full names AND
 *    their components («МОКЕРИ, Psychosis» also matches piecewise;
 *  - `all` — strong + uploader;
 *  - `full` — FULL names only (for gluing «Tyler, The Creator» back together). */
interface KnownNames {
  strong: Set<string>;
  all: Set<string>;
  full: Set<string>;
}

function knownNames(track: DisplayInput): KnownNames {
  const strong = new Set<string>();
  const all = new Set<string>();
  const full = new Set<string>();
  const add = (name: string | undefined | null, isStrong: boolean) => {
    if (!name) return;
    const fullKey = foldName(name);
    if (fullKey) {
      full.add(fullKey);
      all.add(fullKey);
      if (isStrong) strong.add(fullKey);
    }
    for (const part of splitNamesWithOffsets(name)) {
      const pk = foldName(part.text);
      if (!pk) continue;
      all.add(pk);
      if (isStrong) strong.add(pk);
    }
  };
  add(track.enrichment?.primary_artist?.name, true);
  for (const p of track.enrichment?.participants ?? []) add(p.artist?.name, true);
  add(track.user?.username?.trim(), false);
  return { strong, all, full };
}

/** Co-primary artists from enrichment.participants (role='primary') */
export function coPrimaryNames(track: DisplayInput): string[] {
  return (
    track.enrichment?.participants
      ?.filter((p) => p.role === 'primary' && p.artist?.name)
      .map((p) => p.artist.name) ?? []
  );
}

/** Featured artists from enrichment.participants (role='featured') */
export function featuredNames(track: DisplayInput): string[] {
  return (
    track.enrichment?.participants
      ?.filter((p) => p.role === 'featured' && p.artist?.name)
      .map((p) => p.artist.name) ?? []
  );
}

export function dedupeByFold(names: string[]): string[] {
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

/** Glue adjacent pieces back into a known FULL name: the split tore
 *  "Tyler, The Creator" → [Tyler, The Creator], but the raw sub-span as a whole
 *  is in `full` → return it as one name. Greedy, up to 4 pieces. */
function glueKnownAdjacent(raw: string, parts: NamePart[], full: Set<string>): string[] {
  const out: string[] = [];
  let i = 0;
  while (i < parts.length) {
    let matched = -1;
    for (let j = Math.min(parts.length - 1, i + 3); j > i; j--) {
      const span = raw.slice(parts[i].start, parts[j].end);
      if (full.has(foldName(span))) {
        matched = j;
        break;
      }
    }
    if (matched > i) {
      out.push(raw.slice(parts[i].start, parts[matched].end));
      i = matched + 1;
    } else {
      out.push(parts[i].text);
      i++;
    }
  }
  return out;
}

/**
 * The single display parse for a track. Featured artists from enrichment are
 * added to the artist line AFTER the main line-up ("write feat in the artist list").
 */
function computeTrackDisplay(track: DisplayInput): TrackDisplay {
  const base = splitTitleAuthors(track);
  const artistNames = dedupeByFold([...base.artistNames, ...featuredNames(track)]);
  return {
    title: base.title,
    artistNames,
    artistLine: artistNames.join(', '),
    fromTitleSplit: base.fromTitleSplit,
  };
}

/**
 * Split the title into (artists, name).
 *
 * The left side of "… - …" is an artist list when a signal confirms it:
 * at least one name is known from enrichment (strong signal), OR ALL names
 * on the left are known (when the only source is the uploader: «трек, друзья -
 * подпишитесь» must not become «artists» just because it matches the nick).
 * "МОКЕРИ, Psychosis - kill" with «МОКЕРИ» known → artists
 * "МОКЕРИ, Psychosis", name "kill".
 */
function splitTitleAuthors(track: DisplayInput): TrackDisplay {
  const cleaned = stripTranslitParens(stripInlineTags(track.title));
  const known = knownNames(track);

  for (const sep of TITLE_SEPARATORS) {
    const idx = cleaned.indexOf(sep);
    if (idx <= 0) continue;
    const left = cleaned.slice(0, idx).trim();
    const right = cleaned.slice(idx + sep.length).trim();
    if (!right) continue;
    const parts = splitNamesWithOffsets(left);
    if (parts.length === 0 || parts.length > MAX_TITLE_ARTISTS) continue;
    const names = glueKnownAdjacent(left, parts, known.full);
    const confirmed =
      names.some((p) => known.strong.has(foldName(p))) ||
      names.every((p) => known.all.has(foldName(p)));
    if (confirmed) {
      const artistNames = dedupeByFold(names);
      return {
        title: right,
        artistNames,
        artistLine: artistNames.join(', '),
        fromTitleSplit: true,
      };
    }
  }

  // Reversed "Track - Artist" markup (~4% of dashed titles): nothing familiar
  // on the left, fully known artists on the right.
  for (const sep of TITLE_SEPARATORS) {
    const idx = cleaned.indexOf(sep);
    if (idx <= 0) continue;
    const left = cleaned.slice(0, idx).trim();
    const right = cleaned.slice(idx + sep.length).trim();
    if (!left || !right) continue;
    const parts = splitNamesWithOffsets(right);
    if (parts.length === 0 || parts.length > MAX_TITLE_ARTISTS) continue;
    const names = glueKnownAdjacent(right, parts, known.full);
    if (names.every((p) => known.all.has(foldName(p)))) {
      const artistNames = dedupeByFold(names);
      return {
        title: left,
        artistNames,
        artistLine: artistNames.join(', '),
        fromTitleSplit: true,
      };
    }
  }

  // Bare hyphen ("Уннв-Без даты"): cut only when the whole left side is a
  // known artist, otherwise a name like "x-ray" gets torn apart.
  const bareIdx = cleaned.indexOf('-');
  if (bareIdx > 0 && bareIdx + 1 < cleaned.length && cleaned[bareIdx + 1] !== '-') {
    const left = cleaned.slice(0, bareIdx).trim();
    const right = cleaned.slice(bareIdx + 1).trim();
    if (left && right && known.all.has(foldName(left))) {
      return {
        title: right,
        artistNames: [left],
        artistLine: left,
        fromTitleSplit: true,
      };
    }
  }

  const primaryName = track.enrichment?.primary_artist?.name;
  const uploader = track.user?.username?.trim() ?? '';
  const enriched = primaryName ? [primaryName, ...coPrimaryNames(track)] : [];
  const artistNames = dedupeByFold(enriched.length ? enriched : uploader ? [uploader] : []);
  return {
    title: cleaned,
    artistNames,
    artistLine: artistNames.join(', '),
    fromTitleSplit: false,
  };
}

/** Parse cache keyed by track identity (TanStack Query structural sharing keeps
 *  the object stable across renders), revalidated against the input fields.
 *  The result is referentially stable → memo components do not re-render. */
interface CacheEntry {
  title: string;
  user: unknown;
  enrichment: unknown;
  display: TrackDisplay;
}

const DISPLAY_CACHE = new WeakMap<object, CacheEntry>();

export function getTrackDisplay(track: DisplayInput): TrackDisplay {
  const key = track as object;
  const hit = DISPLAY_CACHE.get(key);
  if (
    hit &&
    hit.title === track.title &&
    hit.user === track.user &&
    hit.enrichment === track.enrichment
  ) {
    return hit.display;
  }
  const display = computeTrackDisplay(track);
  DISPLAY_CACHE.set(key, {
    title: track.title,
    user: track.user,
    enrichment: track.enrichment,
    display,
  });
  return display;
}

export function getDisplayTitle(track: DisplayInput): string {
  return getTrackDisplay(track).title;
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
