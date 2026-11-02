import type { EnrichmentArtist, Track } from '../../stores/player';
import { type DisplayInput, getTrackDisplay } from './display';
import { foldName } from './fold';
import { splitNames } from './split';

/** A name in the artist line + where it leads (null — not clickable) */
export interface ArtistLinkItem {
  name: string;
  target: string | null;
}

/**
 * Per-name links for the artist line: every name from `getTrackDisplay`
 * is matched (fold) against enrichment primary / co-primary / participants (→ artist
 * page) or against the uploader (→ user page). Targets are registered both under
 * the full name and under its components — «Tyler, The Creator» stays clickable
 * even when the split tore it into pieces. Unmatched names are plain text.
 */
function computeLinkItems(track: DisplayInput): ArtistLinkItem[] {
  const display = getTrackDisplay(track);
  const targets = new Map<string, string>();
  const put = (name: string | undefined | null, target: string | null) => {
    if (!name || !target) return;
    const fullKey = foldName(name);
    if (fullKey && !targets.has(fullKey)) targets.set(fullKey, target);
    for (const part of splitNames(name)) {
      const pk = foldName(part);
      if (pk && !targets.has(pk)) targets.set(pk, target);
    }
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

interface LinksCacheEntry {
  title: string;
  user: unknown;
  enrichment: unknown;
  items: ArtistLinkItem[];
}

const LINKS_CACHE = new WeakMap<object, LinksCacheEntry>();

export function getArtistLinkItems(track: DisplayInput): ArtistLinkItem[] {
  const key = track as object;
  const hit = LINKS_CACHE.get(key);
  if (
    hit &&
    hit.title === track.title &&
    hit.user === track.user &&
    hit.enrichment === track.enrichment
  ) {
    return hit.items;
  }
  const items = computeLinkItems(track);
  LINKS_CACHE.set(key, {
    title: track.title,
    user: track.user,
    enrichment: track.enrichment,
    items,
  });
  return items;
}

/**
 * Where a click on the "whole artist line" leads (cards where the names are not
 * cracked out individually): the first linked participant — the SAME target
 * as the first name in ArtistNameLinks. This used to have its own verified
 * gate, and the same name led to different places from the list and from the card.
 */
export function getArtistTarget(
  track: Pick<Track, 'title' | 'user' | 'enrichment'>,
): string | null {
  const linked = getArtistLinkItems(track).find((it) => it.target);
  if (linked?.target) return linked.target;
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
