import {
    type ArtistDisplay,
    type DisplayInput,
    getArtistDisplay,
    getDisplayTitle,
    getTrackDisplay,
    type TrackDisplay,
} from './display';
import {type ArtistLinkItem, getArtistLinkItems} from './links';

// Хуки — тонкие обёртки: WeakMap-кэш в display/links делает результат
// референсно стабильным на неизменном треке, useMemo здесь лишний.

export function useTrackDisplay(track: DisplayInput): TrackDisplay {
  return getTrackDisplay(track);
}

export function useArtistDisplay(track: DisplayInput): ArtistDisplay {
  return getArtistDisplay(track);
}

export function useDisplayTitle(track: DisplayInput): string {
  return getDisplayTitle(track);
}

export function useArtistLinkItems(track: DisplayInput): ArtistLinkItem[] {
  return getArtistLinkItems(track);
}
