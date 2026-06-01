import {art} from './formatters';

type CoverTrack = { artwork_url?: string | null };

/** Raw best cover URL for a playlist: its own artwork, else the first track
 *  that has one. The single source of truth for the "playlist with no cover
 *  borrows its first track's art" rule. */
export function rawPlaylistCover(
    artworkUrl: string | null | undefined,
    tracks?: CoverTrack[] | null,
): string | null {
    return artworkUrl || tracks?.find((tr) => tr.artwork_url)?.artwork_url || null;
}

/** Proxied, sized playlist cover with first-track fallback — for an `<img src>`. */
export function playlistCoverUrl(
    artworkUrl: string | null | undefined,
    tracks?: CoverTrack[] | null,
    size = 't500x500',
): string | null {
    return art(rawPlaylistCover(artworkUrl, tracks), size);
}
