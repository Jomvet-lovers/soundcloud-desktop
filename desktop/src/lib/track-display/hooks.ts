import {
  type ArtistDisplay,
  type DisplayInput,
  getArtistDisplay,
  getDisplayTitle,
  getTrackDisplay,
  type TrackDisplay,
} from './display';
import { type ArtistLinkItem, getArtistLinkItems } from './links';

// Hooks are thin wrappers: the WeakMap cache in display/links keeps the result
// referentially stable on an unchanged track, so useMemo is redundant here.

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
