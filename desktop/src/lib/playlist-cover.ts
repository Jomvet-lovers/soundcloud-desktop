import { art } from './formatters';

type CoverTrack = { artwork_url?: string | null };

/** Raw best cover URL for a playlist, in the official web order: its own
 *  artwork, else the first track that has one, else the owner's avatar
 *  (soundcloud.com shows the uploader's avatar for a set with no art at all —
 *  verified against the set pages' og:image). Single source of truth for the
 *  "playlist cover" rule. */
export function rawPlaylistCover(
  artworkUrl: string | null | undefined,
  tracks?: CoverTrack[] | null,
  ownerAvatarUrl?: string | null,
): string | null {
  const own = artworkUrl || tracks?.find((tr) => tr.artwork_url)?.artwork_url;
  return own || ownerAvatarUrl || null;
}

/** Proxied, sized playlist cover with the same fallback chain — for an `<img src>`. */
export function playlistCoverUrl(
  artworkUrl: string | null | undefined,
  tracks?: CoverTrack[] | null,
  size = 't500x500',
  ownerAvatarUrl?: string | null,
): string | null {
  return art(rawPlaylistCover(artworkUrl, tracks, ownerAvatarUrl), size);
}
