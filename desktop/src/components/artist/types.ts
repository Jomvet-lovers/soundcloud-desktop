import type { Track } from '../../stores/player';

export interface ArtistSocial {
  kind: string;
  url: string;
  source: string;
  verified: boolean;
}

export interface ArtistScAccount {
  sc_user_id: string;
  role: string;
  source: string;
  verified: boolean;
}

export interface RelatedArtist {
  id: string;
  name: string;
  country?: string;
  avatar_url?: string;
  weight: number;
}

export interface ArtistDetail {
  id: string;
  name: string;
  country?: string;
  bio?: string;
  avatar_url?: string;
  confidence: number;
  socials: ArtistSocial[];
  sc_accounts: ArtistScAccount[];
  track_count: number;
  track_count_primary: number;
  track_count_featured: number;
  album_count: number;
  popular_tracks: Track[];
  related_artists: RelatedArtist[];
}

export type AlbumKind = 'album' | 'ep' | 'single' | 'compilation' | string;

export interface ArtistAlbum {
  id: string;
  title: string;
  type: AlbumKind;
  release_year?: number;
  cover_url?: string;
  role: string;
}

export type ArtistTabId = 'tracks' | 'appears' | 'albums' | 'related' | 'about';
export type TracksSort = 'popular' | 'recent';
