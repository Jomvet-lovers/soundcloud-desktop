import { type InfiniteData, useInfiniteQuery, useQuery } from '@tanstack/react-query';
import { api } from './api';
import type { AuraId } from './aura';

export type AlbumKind = 'album' | 'ep' | 'single' | 'compilation';
export type ArtistSort = 'trending' | 'listeners' | 'tracks' | 'star' | 'az';
export type AlbumSort = 'recent' | 'popular' | 'tracks' | 'az';
export type AlbumKindFilter = 'all' | AlbumKind;
export type TagFilter = 'all' | string;

export interface CatalogArtist {
  id: string;
  name: string;
  country?: string;
  avatar_url?: string;
  confidence: number;
  track_count_primary: number;
  track_count_featured: number;
  album_count: number;
  monthly_listeners: number;
  trending: number;
  tags: string[];
  star: boolean;
  aura_id?: AuraId | 'custom' | null;
  custom_hex?: string | null;
}

export interface CatalogAlbum {
  id: string;
  title: string;
  type: AlbumKind;
  release_year?: number | null;
  release_month?: number | null;
  cover_url?: string;
  confidence: number;
  primary_artist: {
    id: string;
    name: string;
    avatar_url?: string;
  };
  track_count: number;
  total_duration_ms: number;
  popularity: number;
  star: boolean;
}

export interface DiscoverSummary {
  artists_count: number;
  albums_count: number;
  fresh_count: number;
  fresh_window_days: number;
}

export interface CursorPage<T> {
  items: T[];
  next_cursor?: string | null;
}

const DISCOVER_PAGE_LIMIT = 80;
const DISCOVER_MAX_PAGES = 5;
export const DISCOVER_MIN_SEARCH_LEN = 2;
export const DISCOVER_HARD_CAP = DISCOVER_PAGE_LIMIT * DISCOVER_MAX_PAGES;

const sanitizeSearch = (q: string | undefined): string | undefined => {
  if (!q) return undefined;
  const trimmed = q.trim();
  if (trimmed.length < DISCOVER_MIN_SEARCH_LEN) return undefined;
  return trimmed;
};
const STALE_LIST_MS = 60_000;
const STALE_SUMMARY_MS = 60_000;
const STALE_SPOTLIGHT_MS = 5 * 60_000;

const buildUrl = (path: string, params: Record<string, string | undefined | null>) => {
  const usp = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v == null || v === '') continue;
    usp.set(k, v);
  }
  const qs = usp.toString();
  return qs ? `${path}?${qs}` : path;
};

export interface ArtistsParams {
  sort: ArtistSort;
  tag?: TagFilter;
  q?: string;
}

export interface AlbumsParams {
  sort: AlbumSort;
  kind?: AlbumKindFilter;
  q?: string;
}

export function useDiscoverArtists(params: ArtistsParams) {
  const { sort, tag, q } = params;
  const tagParam = tag && tag !== 'all' ? tag : undefined;
  const search = sanitizeSearch(q);
  return useInfiniteQuery<
    CursorPage<CatalogArtist>,
    Error,
    InfiniteData<CursorPage<CatalogArtist>>,
    readonly [string, string, ArtistSort, string | undefined, string | undefined],
    string | undefined
  >({
    queryKey: ['discover', 'artists', sort, tagParam, search],
    queryFn: ({ pageParam }) =>
      api<CursorPage<CatalogArtist>>(
        buildUrl('/discover/artists', {
          sort,
          tag: tagParam,
          q: search,
          cursor: pageParam,
          limit: String(DISCOVER_PAGE_LIMIT),
        }),
      ),
    initialPageParam: undefined,
    getNextPageParam: (last, allPages) =>
      allPages.length >= DISCOVER_MAX_PAGES ? undefined : (last.next_cursor ?? undefined),
    staleTime: STALE_LIST_MS,
    maxPages: DISCOVER_MAX_PAGES,
  });
}

export function useDiscoverAlbums(params: AlbumsParams) {
  const { sort, kind, q } = params;
  const kindParam = kind && kind !== 'all' ? kind : undefined;
  const search = sanitizeSearch(q);
  return useInfiniteQuery<
    CursorPage<CatalogAlbum>,
    Error,
    InfiniteData<CursorPage<CatalogAlbum>>,
    readonly [string, string, AlbumSort, string | undefined, string | undefined],
    string | undefined
  >({
    queryKey: ['discover', 'albums', sort, kindParam, search],
    queryFn: ({ pageParam }) =>
      api<CursorPage<CatalogAlbum>>(
        buildUrl('/discover/albums', {
          sort,
          kind: kindParam,
          q: search,
          cursor: pageParam,
          limit: String(DISCOVER_PAGE_LIMIT),
        }),
      ),
    initialPageParam: undefined,
    getNextPageParam: (last, allPages) =>
      allPages.length >= DISCOVER_MAX_PAGES ? undefined : (last.next_cursor ?? undefined),
    staleTime: STALE_LIST_MS,
    maxPages: DISCOVER_MAX_PAGES,
  });
}

export interface CatalogTag {
  id: string;
  label: string;
  count: number;
}

export interface YearBucket {
  year: number;
  items: CatalogAlbum[];
}

export interface YearBucketsResponse {
  buckets: YearBucket[];
}

export function useDiscoverAlbumsByYear(opts: {
  years?: number;
  perYear?: number;
  kind?: AlbumKindFilter;
  enabled?: boolean;
}) {
  const years = opts.years ?? 8;
  const perYear = opts.perYear ?? 20;
  const kindParam = opts.kind && opts.kind !== 'all' ? opts.kind : undefined;
  return useQuery<YearBucketsResponse>({
    queryKey: ['discover', 'albumsByYear', years, perYear, kindParam],
    queryFn: () =>
      api<YearBucketsResponse>(
        buildUrl('/discover/albums/by-year', {
          years: String(years),
          per_year: String(perYear),
          kind: kindParam,
        }),
      ),
    staleTime: STALE_LIST_MS,
    enabled: opts.enabled ?? true,
  });
}

export function useDiscoverTags(limit = 12) {
  return useQuery<CursorPage<CatalogTag>>({
    queryKey: ['discover', 'tags', limit],
    queryFn: () =>
      api<CursorPage<CatalogTag>>(buildUrl('/discover/tags', { limit: String(limit) })),
    staleTime: 5 * 60_000,
  });
}

export type SpotlightItem =
  | { kind: 'artist'; artist: CatalogArtist }
  | { kind: 'album'; album: CatalogAlbum };

export interface SpotlightResponse {
  items: SpotlightItem[];
}

export function useDiscoverSpotlight(limit?: number) {
  return useQuery<SpotlightResponse>({
    queryKey: ['discover', 'spotlight', limit ?? 'default'],
    queryFn: () =>
      api<SpotlightResponse>(
        buildUrl('/discover/spotlight', {
          limit: limit != null ? String(limit) : undefined,
        }),
      ),
    staleTime: STALE_SPOTLIGHT_MS,
  });
}

export function useDiscoverSummary() {
  return useQuery<DiscoverSummary>({
    queryKey: ['discover', 'summary'],
    queryFn: () => api<DiscoverSummary>('/discover/summary'),
    staleTime: STALE_SUMMARY_MS,
  });
}

export async function fetchDiscoverRandom(kind: 'album' | 'artist'): Promise<string | null> {
  try {
    const res = await api<{ id: string }>(buildUrl('/discover/random', { type: kind }));
    return res.id ?? null;
  } catch {
    return null;
  }
}

export function flattenPages<T>(data: InfiniteData<CursorPage<T>> | undefined): T[] {
  if (!data) return [];
  const out: T[] = [];
  for (const p of data.pages) {
    if (p?.items) out.push(...p.items);
  }
  return out;
}

export function reachedHardCap<T>(data: InfiniteData<CursorPage<T>> | undefined): boolean {
  if (!data) return false;
  if (data.pages.length < DISCOVER_MAX_PAGES) return false;
  const last = data.pages[data.pages.length - 1];
  return Boolean(last?.next_cursor);
}
