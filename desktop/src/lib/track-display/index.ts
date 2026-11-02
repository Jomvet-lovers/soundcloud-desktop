// Single source of truth for showing a track's artists/title. Parsing — display.ts,
// per-name links — links.ts, comparison keys — fold.ts (mirror of the backend).

export { looksLikeRoleTag, stripInlineTags, stripTranslitParens } from './clean';
export {
  type ArtistDisplay,
  coPrimaryNames,
  type DisplayInput,
  dedupeByFold,
  featuredNames,
  getArtistDisplay,
  getDisplayTitle,
  getTrackDisplay,
  type TrackDisplay,
  type UploadKind,
} from './display';
export { foldName } from './fold';
export { useArtistDisplay, useArtistLinkItems, useDisplayTitle, useTrackDisplay } from './hooks';
export {
  type ArtistLinkItem,
  getArtistLinkItems,
  getArtistTarget,
  getParticipants,
  type ParticipantsBreakdown,
} from './links';
export { splitNames, TITLE_SEPARATORS } from './split';
