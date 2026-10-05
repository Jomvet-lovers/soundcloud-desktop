import {
  type DefaultError,
  type InfiniteData,
  keepPreviousData,
  type QueryClient,
  type QueryKey,
  type UseInfiniteQueryResult,
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query';
import { useEffect, useMemo, useRef } from 'react';
import type { Track } from '../stores/player';
import type { SearchSort } from '../stores/searchPrefs';
import { api } from './api';
import { initLikedUrns } from './likes';
import { rememberLikedTracks, rememberTracks } from './offline-index';
import { fetchRelatedTracks } from './related';

/* ── Types ─────────────────────────────────────────────────────── */

export type FeedOrigin = Track & {
  track_count?: number;
  set_type?: string;
  tracks?: Track[];
};

export interface FeedItem {
  type: string;
  created_at: string;
  origin: FeedOrigin;
}

export interface PagedResponse<T> {
  collection: T[];
  page: number;
  page_size: number;
  has_more: boolean;
}

export interface CatalogArtist {
  id: string;
  name: string;
  avatar_url?: string;
  [key: string]: unknown;
}

export interface CatalogAlbum {
  id: string;
  title: string;
  cover_url?: string;
  [key: string]: unknown;
}

type TrackPage = PagedResponse<Track>;

export interface Comment {
  id: number;
  urn: string;
  body: string;
  created_at: string;
  timestamp: number | null;
  track_id: number;
  user: {
    id: number;
    urn: string;
    username: string;
    avatar_url: string;
    permalink_url: string;
  };
}

export interface Playlist {
  kind?: 'playlist' | 'album' | 'ep' | 'single' | 'compilation';
  id: number;
  urn: string;
  title: string;
  permalink_url?: string;
  description: string | null;
  duration: number;
  artwork_url: string | null;
  genre: string;
  tag_list: string;
  track_count: number;
  likes_count?: number;
  repost_count?: number;
  release_year?: number;
  release_date?: string;
  label_name?: string;
  created_at: string;
  last_modified: string;
  sharing: string;
  playlist_type: string;
  user_favorite?: boolean;
  tracks: Track[];
  user: {
    id: number;
    urn: string;
    username: string;
    avatar_url: string;
    permalink_url?: string;
    followers_count?: number;
    track_count?: number;
  };
}

export interface SCUser {
  id: number;
  urn: string;
  username: string;
  avatar_url: string;
  permalink_url?: string;
  followers_count?: number;
  followings_count?: number;
  track_count?: number;
  city?: string | null;
  /// Backend now emits `country_code` (ISO-2). Legacy `country` оставляем
  /// для совместимости со старыми payload'ами SC.
  country_code?: string | null;
  country?: string | null;
}

export interface UserProfile extends SCUser {
  permalink: string;
  created_at: string;
  last_modified: string;
  first_name: string;
  last_name: string;
  full_name: string;
  description: string | null;
  country: string | null;
  public_favorites_count: number;
  reposts_count: number;
  plan: string;
  website_title: string | null;
  website: string | null;
  comments_count: number;
  online: boolean;
  likes_count: number;
  playlist_count: number;
}

export interface WebProfile {
  id: number;
  kind: string;
  service: string;
  title: string;
  url: string;
  username?: string;
}

const SHORT_CACHE_MS = 1000 * 60 * 2;
const MEDIUM_CACHE_MS = 1000 * 60 * 5;
const SEARCH_CACHE_MS = 1000 * 60 * 2;
const INFINITE_GC_MS = 1000 * 60 * 3;

/**
 * Cold-эндпоинты (треки/плейлисты/лайки/фолловинги юзеров, /me/*) живут в
 * нашей БД и обновляются бэком SWR-cron'ом без участия фронта. tanstack-query
 * не должен сам дёргать refetch на каждый mount — бэк всё равно отдаст cold
 * копию мгновенно. Полагаемся на явные invalidate'ы из мутаций
 * (like/unlike/follow/playlist updates).
 */
const COLD_CACHE_MS = Number.POSITIVE_INFINITY;

/** Page size for the profile likes pagination (page-based, not infinite). */
const USER_LIKES_PAGE_SIZE = 30;

/* ── Helpers ───────────────────────────────────────────────────── */

function flattenCollectionPages<T>(pages: Array<{ collection: T[] }> | undefined): T[] {
  if (!pages) return [];
  const items: T[] = [];
  for (const page of pages) {
    if (!page?.collection) continue;
    items.push(...page.collection);
  }
  return items;
}

export function dedupeByKey<T, K>(items: T[], getKey: (item: T) => K): T[] {
  const seen = new Set<K>();
  const unique: T[] = [];
  for (const item of items) {
    const key = getKey(item);
    if (seen.has(key)) continue;
    seen.add(key);
    unique.push(item);
  }
  return unique;
}

export function dedupeByUrn<T extends { urn: string }>(items: T[]): T[] {
  return dedupeByKey(items, (item) => item.urn);
}

interface PagedQueryOptions<T> {
  queryKey: QueryKey;
  /** Builds the URL for a given page index. limit and page are appended automatically. */
  url: (page: number, limit: number) => string;
  limit?: number;
  staleTime?: number;
  gcTime?: number;
  enabled?: boolean;
  maxPages?: number;
  /** Auto-fetch all pages until exhausted. Use sparingly. */
  autoFetchAll?: boolean;
  dedupe?: (item: T) => string;
}

type PagedQueryResult<T> = UseInfiniteQueryResult<
  InfiniteData<PagedResponse<T>, number>,
  DefaultError
> & { items: T[] };

/**
 * Унифицированный page-based useInfiniteQuery helper. Бэк отдаёт
 * { collection, page, page_size, has_more } — этого достаточно для пагинации.
 */
function usePagedQuery<T>(opts: PagedQueryOptions<T>): PagedQueryResult<T> {
  const limit = opts.limit ?? 30;
  const query = useInfiniteQuery<
    PagedResponse<T>,
    DefaultError,
    InfiniteData<PagedResponse<T>, number>,
    QueryKey,
    number
  >({
    queryKey: opts.queryKey,
    queryFn: ({ pageParam }) => api<PagedResponse<T>>(opts.url(pageParam, limit)),
    initialPageParam: 0,
    getNextPageParam: (last) => (last.has_more ? last.page + 1 : undefined),
    staleTime: opts.staleTime,
    gcTime: opts.gcTime ?? INFINITE_GC_MS,
    maxPages: opts.maxPages,
    enabled: opts.enabled,
    // Списки рефрешатся только явными invalidate'ами из мутаций. Remount/
    // reconnect не должен перетягивать весь infinite-query: для SC cursor-лент
    // это перепроходит сдвинувшийся курсор и тасует выдачу. Focus-рефетч уже
    // выключен глобально в query-client.
    refetchOnMount: false,
    refetchOnReconnect: false,
  });

  // biome-ignore lint/correctness/useExhaustiveDependencies: opts.autoFetchAll is stable, query is captured
  useEffect(() => {
    if (!opts.autoFetchAll) return;
    if (query.hasNextPage && !query.isFetchingNextPage) {
      query.fetchNextPage();
    }
  }, [opts.autoFetchAll, query.hasNextPage, query.isFetchingNextPage, query.data]);

  const items = useMemo(() => {
    const flat = flattenCollectionPages(query.data?.pages);
    return opts.dedupe ? dedupeByKey(flat, opts.dedupe) : flat;
  }, [query.data, opts.dedupe]);

  return Object.assign(query, { items }) as PagedQueryResult<T>;
}

function pagedUrl(base: string, page: number, limit: number, extra?: string): string {
  const sep = base.includes('?') ? '&' : '?';
  const params = `limit=${limit}&page=${page}${extra ? `&${extra}` : ''}`;
  return `${base}${sep}${params}`;
}

/* ── History ───────────────────────────────────────────────────── */

export interface HistoryEntry {
  id: string;
  scTrackId: string;
  title: string;
  artistName: string;
  artistUrn: string | null;
  artworkUrl: string | null;
  duration: number;
  playedAt: string;
}

export function useHistory(limit = 50) {
  const query = useInfiniteQuery({
    queryKey: ['history'],
    queryFn: async ({ pageParam }) => {
      const cursor = pageParam ? `&cursor=${encodeURIComponent(pageParam)}` : '';
      return api<{ collection: HistoryEntry[]; next_cursor: string | null }>(
        `/history?limit=${limit}${cursor}`,
      );
    },
    initialPageParam: undefined as string | undefined,
    gcTime: INFINITE_GC_MS,
    maxPages: 8,
    // SoundCloud paging hands back the next page's query string (from/offset);
    // the local offline fallback returns null and stops here.
    getNextPageParam: (last) => last.next_cursor ?? undefined,
    staleTime: 0,
  });

  const entries = useMemo(() => flattenCollectionPages(query.data?.pages), [query.data]);

  return { entries, ...query };
}

/* ── Featured ─────────────────────────────────────────────────── */

export interface FeaturedResponse {
  type: 'track' | 'playlist' | 'user';
  data: any;
}

export function useFeatured() {
  return useQuery<FeaturedResponse | null>({
    queryKey: ['featured'],
    queryFn: () => api<FeaturedResponse | null>('/featured'),
    staleTime: 5 * 60_000,
  });
}

/* ── Liked tracks ──────────────────────────────────────────────── */

export function useLikedTracks(limit = 30) {
  const query = usePagedQuery<Track>({
    queryKey: ['me', 'likes', 'tracks', limit],
    url: (page, l) => pagedUrl('/me/likes/tracks', page, l),
    limit,
    staleTime: COLD_CACHE_MS,
  });

  const tracks = query.items;

  useEffect(() => {
    if (tracks.length > 0) initLikedUrns(tracks);
  }, [tracks]);

  useEffect(() => {
    if (!query.data) return;
    void rememberLikedTracks(tracks);
  }, [query.data, tracks]);

  return { tracks, ...query };
}

/**
 * Fetch ALL liked tracks. Page-based pagination, shared promise.
 * Optional onPage callback fires per page during the fetch.
 */
let _allLikesPromise: Promise<Track[]> | null = null;

export function fetchAllLikedTracks(
  pageSize = 200,
  onPage?: (tracks: Track[]) => void,
): Promise<Track[]> {
  if (_allLikesPromise && !onPage) return _allLikesPromise;

  const promise = (async () => {
    const all: Track[] = [];
    for (let page = 0; ; page++) {
      const data = await api<TrackPage>(pagedUrl('/me/likes/tracks', page, pageSize));
      for (const t of data.collection) all.push(t);
      void rememberTracks(data.collection);
      onPage?.(data.collection);
      if (!data.has_more) break;
    }
    void rememberLikedTracks(all);
    return all;
  })();

  if (!onPage) {
    _allLikesPromise = promise;
    promise.catch(() => {
      _allLikesPromise = null;
    });
  }

  return promise;
}

export function invalidateAllLikesCache() {
  _allLikesPromise = null;
}

/** Все треки плейлиста, по страницам до конца — под shuffle-continuation. */
export function fetchAllPlaylistTracks(playlistUrn: string, pageSize = 200): Promise<Track[]> {
  return (async () => {
    const all: Track[] = [];
    const base = `/playlists/${encodeURIComponent(playlistUrn)}/tracks`;
    for (let page = 0; ; page++) {
      const data = await api<TrackPage>(pagedUrl(base, page, pageSize));
      for (const t of data.collection) all.push(t);
      void rememberTracks(data.collection);
      if (!data.has_more) break;
    }
    return all;
  })();
}

/* ── Fresh from followed artists ───────────────────────────────── */

export function useFollowingTracks(limit = 20) {
  return useQuery({
    queryKey: ['me', 'followings', 'tracks', limit],
    queryFn: () => api<TrackPage>(`/me/followings/tracks?limit=${limit}&page=0`),
    staleTime: SHORT_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

/* ── Track Comments (infinite) ─────────────────────────────────── */

export function useTrackComments(trackUrn: string | undefined) {
  const query = usePagedQuery<Comment>({
    queryKey: ['track', trackUrn, 'comments'],
    url: (page, limit) =>
      pagedUrl(`/tracks/${encodeURIComponent(trackUrn!)}/comments`, page, limit),
    limit: 20,
    staleTime: SHORT_CACHE_MS,
    maxPages: 6,
    enabled: !!trackUrn,
  });

  return { comments: query.items, ...query };
}

/* ── Post Comment ─────────────────────────────────────────────── */

export function usePostComment(trackUrn: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({ body, timestamp }: { body: string; timestamp?: number }) => {
      return api<Comment>(`/tracks/${encodeURIComponent(trackUrn!)}/comments`, {
        method: 'POST',
        body: JSON.stringify({
          comment: { body, timestamp: timestamp ?? 0 },
        }),
      });
    },
    onSuccess: () => {
      qc.refetchQueries({ queryKey: ['track', trackUrn, 'comments'] });
      qc.refetchQueries({ queryKey: ['track', trackUrn], exact: true });
    },
  });
}

/* ── Related Tracks ───────────────────────────────────────────── */

export function useRelatedTracks(trackUrn: string | undefined, limit = 10) {
  return useQuery({
    queryKey: ['track', trackUrn, 'related', limit],
    queryFn: () => fetchRelatedTracks(trackUrn!, limit),
    enabled: !!trackUrn,
    staleTime: SHORT_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

/* ── Track Favoriters ─────────────────────────────────────────── */

export function useTrackFavoriters(trackUrn: string | undefined, limit = 12) {
  return useQuery({
    queryKey: ['track', trackUrn, 'favoriters', limit],
    queryFn: () =>
      api<PagedResponse<SCUser>>(
        `/tracks/${encodeURIComponent(trackUrn!)}/favoriters?limit=${limit}&page=0`,
      ),
    enabled: !!trackUrn,
    staleTime: SHORT_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

/* ── Playlist Detail (cold) ───────────────────────────────────── */

export function usePlaylist(playlistUrn: string | undefined) {
  return useQuery({
    queryKey: ['playlist', playlistUrn],
    queryFn: () => api<Playlist>(`/playlists/${encodeURIComponent(playlistUrn!)}`),
    enabled: !!playlistUrn,
    staleTime: COLD_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

/* ── Playlist Tracks (cold) ───────────────────────────────────── */

export function usePlaylistTracks(playlistUrn: string | undefined) {
  const query = usePagedQuery<Track>({
    queryKey: ['playlist', playlistUrn, 'tracks'],
    url: (page, limit) =>
      pagedUrl(`/playlists/${encodeURIComponent(playlistUrn!)}/tracks`, page, limit),
    limit: 200,
    staleTime: COLD_CACHE_MS,
    enabled: !!playlistUrn,
    autoFetchAll: true,
  });

  return { tracks: query.items, ...query };
}

/* ── User Profile (cold) ──────────────────────────────────────── */

export function useUser(userUrn: string | undefined) {
  return useQuery({
    queryKey: ['user', userUrn],
    queryFn: () => api<UserProfile>(`/users/${encodeURIComponent(userUrn!)}`),
    enabled: !!userUrn,
    staleTime: COLD_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

export function useUserTracks(userUrn: string | undefined) {
  const query = usePagedQuery<Track>({
    queryKey: ['user', userUrn, 'tracks'],
    url: (page, limit) => pagedUrl(`/users/${encodeURIComponent(userUrn!)}/tracks`, page, limit),
    limit: 30,
    // НЕ cold-infinite: owned-треки переупорядочиваются при новых загрузках
    // артиста, а клиентской мутации (как у like/follow) тут нет — некому слать
    // invalidate. Финитный stale → ремоунт подтянет свежий порядок с бэка.
    staleTime: MEDIUM_CACHE_MS,
    maxPages: 8,
    enabled: !!userUrn,
    dedupe: (t) => t.urn,
  });

  return { tracks: query.items, ...query };
}

export function useUserPopularTracks(userUrn: string | undefined) {
  return useQuery({
    queryKey: ['user', userUrn, 'tracks', 'popular'],
    queryFn: async () => {
      const all: Track[] = [];
      const pageSize = 100;
      for (let page = 0; ; page++) {
        const data = await api<TrackPage>(
          pagedUrl(`/users/${encodeURIComponent(userUrn!)}/tracks`, page, pageSize),
        );
        for (const t of data.collection) all.push(t);
        if (!data.has_more) break;
      }
      all.sort((a, b) => (b.playback_count ?? 0) - (a.playback_count ?? 0));
      return all;
    },
    enabled: !!userUrn,
    staleTime: COLD_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

export function useUserPlaylists(userUrn: string | undefined) {
  const query = usePagedQuery<Playlist>({
    queryKey: ['user', userUrn, 'playlists'],
    url: (page, limit) => pagedUrl(`/users/${encodeURIComponent(userUrn!)}/playlists`, page, limit),
    limit: 30,
    staleTime: COLD_CACHE_MS,
    maxPages: 8,
    enabled: !!userUrn,
    dedupe: (p) => p.urn,
  });

  return { playlists: query.items, ...query };
}

/** Page envelope with an opaque cursor for the next page. */
export interface CursorPage<T> extends PagedResponse<T> {
  next_cursor?: string | null;
}

/**
 * Liked tracks of a profile, one cursor page (30) at a time. SoundCloud's
 * track_likes is cursor-paged — a numeric offset is rejected — so each page is
 * addressed by the previous page's `next_cursor`.
 */
export function userLikedTracksQueryKey(userUrn: string, cursor: string | null) {
  return ['user', userUrn, 'likes', 'tracks', cursor ?? 'first'] as const;
}

export function fetchUserLikedTracksPage(userUrn: string, cursor: string | null) {
  const params = new URLSearchParams({ limit: String(USER_LIKES_PAGE_SIZE) });
  if (cursor) params.set('cursor', cursor);
  return api<CursorPage<Track>>(
    `/users/${encodeURIComponent(userUrn)}/likes/tracks?${params.toString()}`,
  );
}

export function useUserLikedTracksPage(
  userUrn: string | undefined,
  cursor: string | null,
  enabled = true,
) {
  return useQuery({
    queryKey: userLikedTracksQueryKey(userUrn ?? '', cursor),
    queryFn: () => fetchUserLikedTracksPage(userUrn!, cursor),
    enabled: !!userUrn && enabled,
    staleTime: COLD_CACHE_MS,
    // Keep the previous page on screen while the next one arrives.
    placeholderData: keepPreviousData,
  });
}

/** Warm the cache for an adjacent likes page so paging feels instant. */
export function prefetchUserLikedTracksPage(
  queryClient: QueryClient,
  userUrn: string,
  cursor: string | null,
) {
  return queryClient.prefetchQuery({
    queryKey: userLikedTracksQueryKey(userUrn, cursor),
    queryFn: () => fetchUserLikedTracksPage(userUrn, cursor),
    staleTime: COLD_CACHE_MS,
  });
}

export function useUserFollowings(userUrn: string | undefined) {
  const query = usePagedQuery<SCUser>({
    queryKey: ['user', userUrn, 'followings'],
    url: (page, limit) =>
      pagedUrl(`/users/${encodeURIComponent(userUrn!)}/followings`, page, limit),
    limit: 30,
    staleTime: COLD_CACHE_MS,
    maxPages: 8,
    enabled: !!userUrn,
    dedupe: (u) => u.urn,
  });

  return { users: query.items, ...query };
}

/* `/users/{urn}/followers` остался горячим на бэке (входящих подписчиков мы не
 * храним как сущность) — короткий staleTime, как раньше. */
export function useUserFollowers(userUrn: string | undefined) {
  const query = usePagedQuery<SCUser>({
    queryKey: ['user', userUrn, 'followers'],
    url: (page, limit) => pagedUrl(`/users/${encodeURIComponent(userUrn!)}/followers`, page, limit),
    limit: 30,
    staleTime: SHORT_CACHE_MS,
    maxPages: 8,
    enabled: !!userUrn,
    dedupe: (u) => u.urn,
  });

  return { users: query.items, ...query };
}

export function useUserWebProfiles(userUrn: string | undefined) {
  return useQuery({
    queryKey: ['user', userUrn, 'web-profiles'],
    queryFn: () => api<WebProfile[]>(`/users/${encodeURIComponent(userUrn!)}/web-profiles`),
    enabled: !!userUrn,
    staleTime: MEDIUM_CACHE_MS,
    gcTime: INFINITE_GC_MS,
  });
}

export function useUserSubscription(userUrn: string | undefined) {
  return useQuery({
    queryKey: ['user', userUrn, 'subscription'],
    queryFn: () => api<{ premium: boolean }>(`/users/${encodeURIComponent(userUrn!)}/subscription`),
    enabled: !!userUrn,
    staleTime: MEDIUM_CACHE_MS,
    gcTime: INFINITE_GC_MS,
    select: (d) => d.premium,
  });
}

/* ── My Library (cold) ─────────────────────────────────────────── */

export function useMyFollowings(limit = 30) {
  const query = usePagedQuery<SCUser>({
    queryKey: ['me', 'followings', limit],
    url: (page, l) => pagedUrl('/me/followings', page, l),
    limit,
    staleTime: COLD_CACHE_MS,
  });

  return { users: query.items, ...query };
}

export function useMyLikedPlaylists(limit = 30) {
  const query = usePagedQuery<Playlist>({
    queryKey: ['me', 'likes', 'playlists', limit],
    url: (page, l) => pagedUrl('/me/likes/playlists', page, l),
    limit,
    staleTime: COLD_CACHE_MS,
  });

  return { playlists: query.items, ...query };
}

export function useMyPlaylists(limit = 30) {
  const query = usePagedQuery<Playlist>({
    queryKey: ['me', 'playlists', limit],
    url: (page, l) => pagedUrl('/me/playlists', page, l),
    limit,
    staleTime: COLD_CACHE_MS,
  });

  return { playlists: query.items, ...query };
}

/* ── Playlist Mutations ────────────────────────────────────────── */

// Полная перестановка/удаление из свежей загруженной вью — шлём `{order}`-дельту
// (а не PUT всего списка): backend применяет к desired-state и пушит в SC фоном.
export function useUpdatePlaylistTracks(playlistUrn: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (trackUrns: string[]) =>
      api(`/playlists/${encodeURIComponent(playlistUrn!)}/tracks`, {
        method: 'POST',
        body: JSON.stringify({ order: trackUrns }),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['playlist', playlistUrn] });
      qc.invalidateQueries({ queryKey: ['playlist', playlistUrn, 'tracks'] });
      qc.invalidateQueries({ queryKey: ['me', 'playlists'] });
    },
  });
}

// Добавление — `{add}`-дельты (по одной на трек). Backend дедупит и считает
// дельту против сохранённого desired-state, поэтому устаревшая клиентская вью
// НЕ может уронить уже лежащие треки (прежний full-list PUT мог).
export function useAddToPlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: async ({
      playlistUrn,
      trackUrns,
    }: {
      playlistUrn: string;
      trackUrns: string[];
    }) => {
      let last: unknown;
      for (const urn of trackUrns) {
        last = await api(`/playlists/${encodeURIComponent(playlistUrn)}/tracks`, {
          method: 'POST',
          body: JSON.stringify({ add: urn }),
        });
      }
      return last;
    },
    onSuccess: (_data, vars) => {
      qc.invalidateQueries({ queryKey: ['playlist', vars.playlistUrn] });
      qc.invalidateQueries({ queryKey: ['playlist', vars.playlistUrn, 'tracks'] });
      qc.invalidateQueries({ queryKey: ['me', 'playlists'] });
    },
  });
}

export function useCreatePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (params: { title: string; sharing?: 'public' | 'private'; trackUrns?: string[] }) =>
      api<Playlist>('/playlists', {
        method: 'POST',
        body: JSON.stringify({
          playlist: {
            title: params.title,
            sharing: params.sharing ?? 'public',
            ...(params.trackUrns?.length
              ? { tracks: params.trackUrns.map((urn) => ({ urn })) }
              : {}),
          },
        }),
      }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['me', 'playlists'] });
    },
  });
}

/* ── Sharing (privacy) ─────────────────────────────────────────── */

/** Тоггл приватности своего плейлиста. Optimistic: бэк сразу обновляет наш
 *  `sharing` + кладёт write-back в SC через sync_queue. */
export function useSetPlaylistSharing(playlistUrn: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sharing: 'public' | 'private') =>
      api(`/playlists/${encodeURIComponent(playlistUrn!)}/sharing`, {
        method: 'PUT',
        body: JSON.stringify({ sharing }),
      }),
    onSuccess: (_data, sharing) => {
      qc.setQueryData<Playlist>(['playlist', playlistUrn], (old) =>
        old ? { ...old, sharing } : old,
      );
      qc.invalidateQueries({ queryKey: ['playlist', playlistUrn] });
      qc.invalidateQueries({ queryKey: ['me', 'playlists'] });
      // Список своих плейлистов на профиле — ['user', urn, 'playlists'].
      qc.invalidateQueries({ queryKey: ['user'] });
    },
  });
}

/** Тоггл приватности своего трека. */
export function useSetTrackSharing(trackUrn: string | undefined) {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (sharing: 'public' | 'private') =>
      api(`/tracks/${encodeURIComponent(trackUrn!)}/sharing`, {
        method: 'PUT',
        body: JSON.stringify({ sharing }),
      }),
    onSuccess: (_data, sharing) => {
      qc.setQueryData<Track>(['track', trackUrn], (old) => (old ? { ...old, sharing } : old));
      qc.invalidateQueries({ queryKey: ['track', trackUrn], exact: true });
      // Списки своих треков на профиле — ['user', urn, 'tracks'] (нет ['me','tracks']).
      qc.invalidateQueries({ queryKey: ['user'] });
    },
  });
}

export function useDeletePlaylist() {
  const qc = useQueryClient();
  return useMutation({
    mutationFn: (playlistUrn: string) =>
      api(`/playlists/${encodeURIComponent(playlistUrn)}`, { method: 'DELETE' }),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['me', 'playlists'] });
    },
  });
}

/* ── Search ────────────────────────────────────────────────────── */

export function useSearchTracks(q: string) {
  const query = usePagedQuery<Track>({
    queryKey: ['search', 'tracks', q],
    url: (page, limit) => pagedUrl('/tracks', page, limit, `q=${encodeURIComponent(q)}`),
    limit: 20,
    staleTime: SEARCH_CACHE_MS,
    maxPages: 5,
    enabled: !!q.trim(),
    dedupe: (t) => t.urn,
  });

  return { tracks: query.items, ...query };
}

export function useSearchPlaylists(q: string) {
  const query = usePagedQuery<Playlist>({
    queryKey: ['search', 'playlists', q],
    url: (page, limit) => pagedUrl('/playlists', page, limit, `q=${encodeURIComponent(q)}`),
    limit: 20,
    staleTime: SEARCH_CACHE_MS,
    maxPages: 5,
    enabled: !!q.trim(),
    dedupe: (p) => p.urn,
  });

  return { playlists: query.items, ...query };
}

export function useSearchUsers(q: string) {
  const query = usePagedQuery<SCUser>({
    queryKey: ['search', 'users', q],
    url: (page, limit) => pagedUrl('/users', page, limit, `q=${encodeURIComponent(q)}`),
    limit: 20,
    staleTime: SEARCH_CACHE_MS,
    maxPages: 5,
    enabled: !!q.trim(),
    dedupe: (u) => u.urn,
  });

  return { users: query.items, ...query };
}

/* ── Search: SCD-DB ───────────────────────────────────────────── */

/**
 * Поиск в нашей базе (зеркало SoundCloud). Возвращает только то, что мы уже
 * индексировали — но без сетевого fan-out'а в SC API, поэтому в разы быстрее.
 * Бэк зашит на trgm-индексы + statement_timeout, фронту достаточно поднести
 * `q` и опционально `userUrn` для скоупа.
 */

const SEARCH_DB_LIMIT = 20;
const SEARCH_DB_MAX_PAGES = 10;
/** Items per page for the numbered pagination on the Search page. */
const SEARCH_DB_PAGE_SIZE = 25;

function searchDbExtra(userUrn?: string, sort?: SearchSort): string {
  return [
    userUrn ? `user_urn=${encodeURIComponent(userUrn)}` : '',
    sort && sort !== 'relevance' ? `sort=${sort}` : '',
  ]
    .filter(Boolean)
    .join('&');
}

function searchDbUrl(kind: string, page: number, limit: number, q: string, extra: string): string {
  return pagedUrl(
    `/search/db/${kind}`,
    page,
    limit,
    `q=${encodeURIComponent(q)}${extra ? `&${extra}` : ''}`,
  );
}

export function useSearchDbTracks(q: string, userUrn?: string, sort: SearchSort = 'relevance') {
  const extra = searchDbExtra(userUrn, sort);
  const query = usePagedQuery<Track>({
    queryKey: ['search', 'db', 'tracks', q, userUrn ?? '', sort],
    url: (page, limit) => searchDbUrl('tracks', page, limit, q, extra),
    limit: SEARCH_DB_LIMIT,
    staleTime: SEARCH_CACHE_MS,
    maxPages: SEARCH_DB_MAX_PAGES,
    enabled: !!q.trim(),
    dedupe: (t) => t.urn,
  });
  return { tracks: query.items, ...query };
}

export function useSearchDbPlaylists(q: string, userUrn?: string) {
  const extra = searchDbExtra(userUrn);
  const query = usePagedQuery<Playlist>({
    queryKey: ['search', 'db', 'playlists', q, userUrn ?? ''],
    url: (page, limit) => searchDbUrl('playlists', page, limit, q, extra),
    limit: SEARCH_DB_LIMIT,
    staleTime: SEARCH_CACHE_MS,
    maxPages: SEARCH_DB_MAX_PAGES,
    enabled: !!q.trim(),
    dedupe: (p) => p.urn,
  });
  return { playlists: query.items, ...query };
}

export function useSearchDbUsers(q: string) {
  const query = usePagedQuery<SCUser>({
    queryKey: ['search', 'db', 'users', q],
    url: (page, limit) => searchDbUrl('users', page, limit, q, searchDbExtra()),
    limit: SEARCH_DB_LIMIT,
    staleTime: SEARCH_CACHE_MS,
    maxPages: SEARCH_DB_MAX_PAGES,
    enabled: !!q.trim(),
    dedupe: (u) => u.urn,
  });
  return { users: query.items, ...query };
}

export function useSearchDbArtists(q: string) {
  const query = usePagedQuery<CatalogArtist>({
    queryKey: ['search', 'db', 'artists', q],
    url: (page, limit) => searchDbUrl('artists', page, limit, q, searchDbExtra()),
    limit: SEARCH_DB_LIMIT,
    staleTime: SEARCH_CACHE_MS,
    maxPages: SEARCH_DB_MAX_PAGES,
    enabled: !!q.trim(),
    dedupe: (a) => a.id,
  });
  return { artists: query.items, ...query };
}

export function useSearchDbAlbums(q: string) {
  const query = usePagedQuery<CatalogAlbum>({
    queryKey: ['search', 'db', 'albums', q],
    url: (page, limit) => searchDbUrl('albums', page, limit, q, searchDbExtra()),
    limit: SEARCH_DB_LIMIT,
    staleTime: SEARCH_CACHE_MS,
    maxPages: SEARCH_DB_MAX_PAGES,
    enabled: !!q.trim(),
    dedupe: (a) => a.id,
  });
  return { albums: query.items, ...query };
}

/** Single-page variant (numbered pagination) — used by the Search page. */
function useSearchDbPage<T>(kind: string, q: string, page: number, extra: string) {
  const query = useQuery<PagedResponse<T>>({
    queryKey: ['search', 'db', kind, q, extra, page],
    queryFn: () => api<PagedResponse<T>>(searchDbUrl(kind, page, SEARCH_DB_PAGE_SIZE, q, extra)),
    staleTime: SEARCH_CACHE_MS,
    placeholderData: keepPreviousData,
    refetchOnMount: false,
    refetchOnReconnect: false,
    enabled: !!q.trim(),
  });
  return {
    items: query.data?.collection ?? [],
    hasMore: query.data?.has_more === true,
    ...query,
  };
}

export function useSearchDbTracksPage(
  q: string,
  page: number,
  userUrn?: string,
  sort: SearchSort = 'relevance',
) {
  const extra = searchDbExtra(userUrn, sort);
  const query = useSearchDbPage<Track>('tracks', q, page, extra);
  return { tracks: query.items, ...query };
}

export function useSearchDbPlaylistsPage(q: string, page: number, userUrn?: string) {
  const extra = searchDbExtra(userUrn);
  const query = useSearchDbPage<Playlist>('playlists', q, page, extra);
  return { playlists: query.items, ...query };
}

export function useSearchDbUsersPage(q: string, page: number) {
  const query = useSearchDbPage<SCUser>('users', q, page, searchDbExtra());
  return { users: query.items, ...query };
}

export function useSearchDbAlbumsPage(q: string, page: number) {
  const query = useSearchDbPage<CatalogAlbum>('albums', q, page, searchDbExtra());
  return { albums: query.items, ...query };
}

/* ── Search: Vibe + Lyrics (AI) ───────────────────────────────── */

const EMPTY_TRACKS: Track[] = [];
const EMPTY_ATMOSPHERE: SearchAtmosphere = { topGenres: [] };

export interface SearchAtmosphere {
  /** Dominant genres of the result set — used to tint the page atmosphere. */
  topGenres: string[];
}

export interface VibeSearchResponse {
  items: Track[];
  atmosphere: SearchAtmosphere;
  /** "preparing" = the query vector is still being computed by the worker
   *  (high load); items is empty, the UI shows a "preparing vibe" plaque and
   *  this query auto-refetches until it flips to "ready". */
  status?: 'ready' | 'preparing';
}

/**
 * Semantic "by vibe" search. Backend encodes the query (MuLan→CLAP, cached) and
 * returns SC-shaped tracks in similarity order plus an `atmosphere` hint
 * (dominant genres) the UI uses to recolour the page.
 */
export function useVibeSearch(q: string, opts?: { limit?: number; languages?: string[] }) {
  const limit = opts?.limit ?? 48;
  const langs = (opts?.languages ?? []).slice().sort().join(',');
  const query = useQuery({
    queryKey: ['search', 'vibe', q, limit, langs],
    enabled: q.trim().length >= 2,
    staleTime: SEARCH_CACHE_MS,
    // While the worker is still encoding the query (preparing), poll until the
    // vector lands and the backend flips to ready.
    refetchInterval: (q2) => (q2.state.data?.status === 'preparing' ? 2500 : false),
    queryFn: () => {
      const usp = new URLSearchParams({ q: q.trim(), limit: String(limit) });
      if (langs) usp.set('languages', langs);
      return api<VibeSearchResponse>(`/search/vibe?${usp}`, undefined, 30_000);
    },
  });
  return {
    tracks: query.data?.items ?? EMPTY_TRACKS,
    atmosphere: query.data?.atmosphere ?? EMPTY_ATMOSPHERE,
    preparing: query.data?.status === 'preparing',
    ...query,
  };
}

export type LyricMode = 'text' | 'semantic' | 'auto';

export interface LyricHit {
  track: Track;
  /** The matched lyric line (text mode); null for pure semantic hits. */
  matchedLine: string | null;
  score: number;
}

/**
 * Lyric search. `text` = keyword match over stored lyrics (returns the matched
 * line); `semantic` = lyric-embedding similarity; `auto` = both, merged.
 */
export function useLyricSearch(q: string, mode: LyricMode = 'auto') {
  const query = usePagedQuery<LyricHit>({
    queryKey: ['search', 'lyrics', q, mode],
    url: (page, limit) =>
      pagedUrl('/search/lyrics', page, limit, `q=${encodeURIComponent(q)}&mode=${mode}`),
    limit: SEARCH_DB_LIMIT,
    staleTime: SEARCH_CACHE_MS,
    maxPages: SEARCH_DB_MAX_PAGES,
    enabled: q.trim().length >= 2,
    dedupe: (h) => h.track.urn,
  });
  return { hits: query.items, ...query };
}

/* ── Fallback / Seed Tracks ────────────────────────────────────── */

const FALLBACK_TRACK_IDS = '2028682452,2065341288,2028677636,2209249766,2060818444,2064016848';

export function useFallbackTracks() {
  return useQuery({
    queryKey: ['fallback', 'tracks'],
    queryFn: () => api<TrackPage>(`/tracks?ids=${FALLBACK_TRACK_IDS}&page=0&limit=30`),
    staleTime: 1000 * 60 * 30,
  });
}

/* ── Discover ──────────────────────────────────────────────────── */

type RelatedPool = Map<string, { count: number; track: Track }>;

function sampleTrackUrns(tracks: Track[], limit: number): string[] {
  if (tracks.length <= limit) {
    return tracks.map((track) => track.urn);
  }

  const sample = tracks.slice(0, limit);
  for (let i = limit; i < tracks.length; i++) {
    const swapIndex = Math.floor(Math.random() * (i + 1));
    if (swapIndex < limit) {
      sample[swapIndex] = tracks[i];
    }
  }

  return sample.map((track) => track.urn);
}

/**
 * Shared pool: fetches related tracks for up to 30 random liked tracks,
 * counts frequency of each related track. Used by both Recommended and Discover.
 */
export function useRelatedPool(likedTracks: Track[]) {
  // Stable seed — compute once when liked tracks first arrive, don't recompute on likes
  const seedRef = useRef<string[]>([]);
  if (seedRef.current.length === 0 && likedTracks.length > 0) {
    seedRef.current = sampleTrackUrns(likedTracks, 30);
  }
  const seedUrns = seedRef.current;

  const likedUrns = useMemo(() => new Set(likedTracks.map((t) => t.urn)), [likedTracks]);

  return useQuery({
    queryKey: ['discover', 'related-pool', seedUrns],
    queryFn: async () => {
      const results = await Promise.all(
        seedUrns.map((urn) =>
          fetchRelatedTracks(urn, 20).catch(
            () => ({ collection: [], page: 0, page_size: 20, has_more: false }) as TrackPage,
          ),
        ),
      );

      const freq: RelatedPool = new Map();
      for (const res of results) {
        for (const track of res.collection) {
          if (likedUrns.has(track.urn)) continue;
          const entry = freq.get(track.urn);
          if (entry) entry.count++;
          else freq.set(track.urn, { count: 1, track });
        }
      }
      return freq;
    },
    enabled: seedUrns.length > 0,
    staleTime: 1000 * 60 * 10,
    gcTime: INFINITE_GC_MS,
  });
}

/** Top related tracks sorted by frequency — "Recommended For You" */
export function useRecommendedTracks(pool: RelatedPool | undefined, limit = 40) {
  return useMemo(() => {
    if (!pool) return [];
    return [...pool.values()]
      .sort((a, b) => b.count - a.count)
      .slice(0, limit)
      .map((e) => e.track);
  }, [pool, limit]);
}

/** Related tracks grouped by genre, sorted by frequency — "Discover" */
export function useDiscoverData(pool: RelatedPool | undefined, likedTracks: Track[]) {
  const genreRanking = useMemo(() => {
    const counts = new Map<string, number>();
    for (const t of likedTracks) {
      const g = t.genre?.trim().toLowerCase();
      if (g) counts.set(g, (counts.get(g) ?? 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([g]) => g);
  }, [likedTracks]);

  return useMemo(() => {
    if (!pool) return [];

    const byGenre = new Map<string, { count: number; track: Track }[]>();
    for (const entry of pool.values()) {
      const g = entry.track.genre?.trim().toLowerCase();
      if (!g) continue;
      const arr = byGenre.get(g);
      if (arr) arr.push(entry);
      else byGenre.set(g, [entry]);
    }

    for (const arr of byGenre.values()) {
      arr.sort((a, b) => b.count - a.count);
    }

    const result: { genre: string; tracks: Track[] }[] = [];
    for (const genre of genreRanking) {
      const entries = byGenre.get(genre);
      if (!entries || entries.length <= 3) continue;
      result.push({ genre, tracks: entries.map((e) => e.track) });
      if (result.length >= 7) break;
    }

    return result;
  }, [pool, genreRanking]);
}

/**
 * Общий related-pool фид: рекомендации + дискавери по жанрам, всё из лайков
 * зрителя. Только данные (без рендера), чтобы полка «Recommended» на Home и
 * призма Discover читали один источник, а не пересобирали пул каждая у себя.
 */
export function useDiscoverFeed() {
  const { tracks: likedTracks } = useLikedTracks(100);
  const { data: pool, isLoading } = useRelatedPool(likedTracks);
  const recommended = useRecommendedTracks(pool, 40);
  const byGenre = useDiscoverData(pool, likedTracks);
  return { likedTracks, isLoading, recommended, byGenre };
}

/* ── SoundCloud mixed selections ("Made for you" / stations) ───── */

export interface MixedSelectionItem {
  urn: string;
  kind?: string | null;
  title: string;
  short_title?: string | null;
  description?: string | null;
  short_description?: string | null;
  artwork_url?: string | null;
  calculated_artwork_url?: string | null;
  playlist_type?: string | null;
  tracking_feature_name?: string | null;
  user?: { urn?: string; id?: number; username?: string } | null;
}

export interface MixedSelection {
  urn: string;
  title: string;
  description?: string | null;
  items: { collection: MixedSelectionItem[] };
}

/** Personalized SoundCloud selections: Made for you, Discover with Stations, … */
export function useDiscoverMixed() {
  return useQuery({
    queryKey: ['discover', 'mixed'],
    queryFn: () => api<{ collection: MixedSelection[] }>('/discover/mixed'),
    staleTime: 1000 * 60 * 10,
    gcTime: INFINITE_GC_MS,
  });
}

/** Tracks of a SoundCloud system playlist (Daily Drops, Weekly Wave, …). */
export async function fetchSystemPlaylistTracks(urn: string): Promise<Track[]> {
  const pl = await api<{ tracks?: Track[] | { collection?: Track[] } }>(
    `/system-playlists/${encodeURIComponent(urn)}`,
  );
  const raw = pl.tracks;
  const arr = Array.isArray(raw) ? raw : (raw?.collection ?? []);
  return arr.filter((t) => t?.urn && (t as { kind?: string }).kind !== 'playlist');
}

/* ── Infinite scroll ───────────────────────────────────────────── */

export function useInfiniteScroll(
  hasNextPage: boolean,
  isFetchingNextPage: boolean,
  fetchNextPage: () => void,
) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el || !hasNextPage || isFetchingNextPage) return;

    const root = el.closest('main');

    const observer = new IntersectionObserver(
      ([entry]) => {
        if (entry.isIntersecting) {
          fetchNextPage();
        }
      },
      { root, rootMargin: '400px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  return ref;
}
