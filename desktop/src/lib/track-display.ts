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

export type UploadKind = 'original' | 'demo' | 'alt' | 'reupload' | 'unknown';

const TITLE_SEPARATORS = [' - ', ' — ', ' – ', ' -- '] as const;

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

export function getDisplayTitle(track: Pick<Track, 'title' | 'enrichment'>): string {
  const real = track.enrichment?.primary_artist;
  if (!real?.verified || !real.name) return track.title;
  const albumTitle = track.enrichment?.album?.title;
  const realName = real.name.trim().toLowerCase();
  for (const sep of TITLE_SEPARATORS) {
    const idx = track.title.indexOf(sep);
    if (idx > 0) {
      const left = track.title.slice(0, idx).trim();
      if (left.toLowerCase() === realName) {
        const right = track.title.slice(idx + sep.length).trim();
        if (albumTitle && right.toLowerCase() === albumTitle.trim().toLowerCase()) {
          return right;
        }
        return right;
      }
    }
  }
  return track.title;
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

export function useDisplayTitle(track: Pick<Track, 'title' | 'enrichment'>): string {
  return useMemo(
    () => getDisplayTitle(track),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      track.title,
      track.enrichment?.primary_artist?.name,
      track.enrichment?.primary_artist?.verified,
      track.enrichment?.album?.title,
    ],
  );
}

export interface ParticipantsBreakdown {
  featured: EnrichmentArtist[];
  remixers: EnrichmentArtist[];
}

export function getParticipants(
  track: Pick<Track, 'enrichment'>,
  roles: ReadonlyArray<string> = ['featured', 'remixer'],
): ParticipantsBreakdown | null {
  const items = track.enrichment?.participants?.filter((p) => roles.includes(p.role)) ?? [];
  if (items.length === 0) return null;
  const featured = items.filter((p) => p.role === 'featured').map((p) => p.artist);
  const remixers = items.filter((p) => p.role === 'remixer').map((p) => p.artist);
  if (featured.length === 0 && remixers.length === 0) return null;
  return { featured, remixers };
}
