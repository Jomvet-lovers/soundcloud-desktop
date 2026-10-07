import { useQueryClient } from '@tanstack/react-query';
import React, { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import type { Aura } from '../../lib/aura';
import { fc } from '../../lib/formatters';
import {
  prefetchUserLikedTracksPage,
  useInfiniteScroll,
  useSearchDbPlaylists,
  useSearchDbTracks,
  useUserFollowers,
  useUserFollowings,
  useUserLikedTracksPage,
  useUserPlaylists,
  useUserPopularTracks,
  useUserTracks,
} from '../../lib/hooks';
import { ChevronLeft, ChevronRight, Loader2, Music } from '../../lib/icons';
import type { Track } from '../../stores/player';
import { PlaylistCard } from '../music/PlaylistCard';
import { Avatar } from '../ui/Avatar';
import { VirtualGrid } from '../ui/VirtualGrid';
import { VirtualList } from '../ui/VirtualList';
import { ProfileLikesPlayButton } from './ProfileLikesPlayButton';
import { ThemedTrackRow } from './ThemedTrackRow';

interface TabWrapperProps {
  isLoading: boolean;
  isEmpty: boolean;
  emptyText?: string;
  children: React.ReactNode;
}

function TabWrapperImpl({ children, isLoading, isEmpty, emptyText }: TabWrapperProps) {
  return (
    <div className="min-h-[420px]">
      {isLoading ? (
        <div className="py-24 flex justify-center">
          <Loader2 size={28} className="text-white/20 animate-spin" />
        </div>
      ) : isEmpty ? (
        <div className="py-24 flex flex-col items-center gap-3">
          <Music size={28} className="text-white/15" />
          <p className="text-white/30 text-sm">{emptyText ?? 'Nothing here yet'}</p>
        </div>
      ) : (
        <div className="animate-soft-in">{children}</div>
      )}
    </div>
  );
}

export const TabWrapper = React.memo(TabWrapperImpl);

export function UserTracksTab({ urn, aura }: { urn: string; aura: Aura }) {
  const q = useUserTracks(urn);
  const ref = useInfiniteScroll(!!q.hasNextPage, !!q.isFetchingNextPage, q.fetchNextPage);
  const renderItem = useCallback(
    (track: (typeof q.tracks)[number], i: number) => (
      <ThemedTrackRow track={track} index={i} queue={q.tracks} aura={aura} />
    ),
    [aura, q.tracks],
  );
  return (
    <TabWrapper isLoading={q.isLoading} isEmpty={q.tracks.length === 0}>
      <VirtualList
        items={q.tracks}
        rowHeight={64}
        overscan={8}
        className="flex flex-col gap-1"
        getItemKey={(t) => t.urn}
        renderItem={renderItem}
      />
      <div ref={ref} className="h-16 flex items-center justify-center">
        {q.isFetchingNextPage && <Loader2 size={20} className="text-white/20 animate-spin" />}
      </div>
    </TabWrapper>
  );
}

export function UserPopularTab({ urn, aura }: { urn: string; aura: Aura }) {
  const { data = [], isLoading } = useUserPopularTracks(urn);
  const renderItem = useCallback(
    (track: (typeof data)[number], i: number) => (
      <ThemedTrackRow track={track} index={i} queue={data} aura={aura} />
    ),
    [aura, data],
  );
  return (
    <TabWrapper isLoading={isLoading} isEmpty={data.length === 0}>
      <VirtualList
        items={data}
        rowHeight={64}
        overscan={8}
        className="flex flex-col gap-1"
        getItemKey={(t) => t.urn}
        renderItem={renderItem}
      />
    </TabWrapper>
  );
}

export function UserPlaylistsTab({ urn }: { urn: string }) {
  const q = useUserPlaylists(urn);
  const ref = useInfiniteScroll(!!q.hasNextPage, !!q.isFetchingNextPage, q.fetchNextPage);
  const renderItem = useCallback(
    (p: (typeof q.playlists)[number]) => <PlaylistCard playlist={p} showPlayback />,
    [],
  );
  return (
    <TabWrapper isLoading={q.isLoading} isEmpty={q.playlists.length === 0}>
      <VirtualGrid
        items={q.playlists}
        itemHeight={250}
        minColumnWidth={140}
        gap={16}
        overscan={3}
        getItemKey={(p, i) => `${p.urn}-${i}`}
        renderItem={renderItem}
      />
      <div ref={ref} className="h-16 flex items-center justify-center">
        {q.isFetchingNextPage && <Loader2 size={20} className="text-white/20 animate-spin" />}
      </div>
    </TabWrapper>
  );
}

const pageBtn =
  'flex size-8 items-center justify-center rounded-full text-white/55 transition-colors hover:bg-white/[0.06] hover:text-white/90 disabled:cursor-default disabled:opacity-30 disabled:hover:bg-transparent';

/**
 * Profile likes, one cursor page (30) at a time. SoundCloud's track_likes is
 * cursor-paged (numeric offsets are rejected), so the cursor that opens each
 * page is remembered; adjacent pages are prefetched to make prev/next instant.
 */
export function UserLikesTab({ urn, aura }: { urn: string; aura: Aura }) {
  const queryClient = useQueryClient();
  // Remounted via `key={urn}` from the page, so a new profile starts fresh.
  const [page, setPage] = useState(0);
  const [cursors, setCursors] = useState<Array<string | null>>([null]);

  const q = useUserLikedTracksPage(urn, cursors[page] ?? null);
  const tracks = q.data?.collection ?? [];
  const nextCursor = q.data?.next_cursor ?? null;
  // SoundCloud reports `next_href` even on the last page, so probe the next
  // page: an empty probe means there is nothing more to show.
  const next = useUserLikedTracksPage(urn, nextCursor, !!nextCursor);
  const canPrev = page > 0;
  const canNext = !!nextCursor && (next.data?.collection.length ?? 0) > 0;

  // Row numbers keep counting across pages (1…30, 31…60, …).
  const [counts, setCounts] = useState<number[]>([]);
  useEffect(() => {
    if (q.isPlaceholderData) return;
    const n = q.data?.collection.length;
    if (n === undefined) return;
    setCounts((prev) => (prev[page] === n ? prev : [...prev.slice(0, page), n]));
  }, [q.data, q.isPlaceholderData, page]);
  const base = counts.slice(0, page).reduce((sum, n) => sum + n, 0);

  // Remember the cursor that opens the next page.
  useEffect(() => {
    if (!nextCursor) return;
    setCursors((prev) => {
      if (prev[page + 1] === nextCursor) return prev;
      const next = prev.slice(0, page + 1);
      next[page + 1] = nextCursor;
      return next;
    });
  }, [nextCursor, page]);

  // Preload the previous page (the next one is already probed above).
  useEffect(() => {
    if (!urn || page === 0) return;
    void prefetchUserLikedTracksPage(queryClient, urn, cursors[page - 1] ?? null);
  }, [urn, page, cursors, queryClient]);

  const renderItem = useCallback(
    (track: Track, i: number) => (
      <ThemedTrackRow track={track} index={base + i} queue={tracks} aura={aura} />
    ),
    [aura, tracks, base],
  );

  return (
    <TabWrapper isLoading={q.isLoading && tracks.length === 0} isEmpty={tracks.length === 0}>
      {/* Play all — top-right, mirroring the other collection pages. */}
      <div className="mb-2 flex items-center justify-end px-4">
        <ProfileLikesPlayButton urn={urn} />
      </div>
      <div key={page} className="animate-soft-in">
        <VirtualList
          items={tracks}
          rowHeight={64}
          overscan={8}
          className="flex flex-col gap-1"
          getItemKey={(t) => t.urn}
          renderItem={renderItem}
        />
      </div>
      {(canPrev || nextCursor) && (
        <div className="mt-1 flex items-center justify-center gap-2 border-t border-white/[0.05] pt-2">
          <button
            type="button"
            disabled={!canPrev}
            onClick={() => setPage((p) => Math.max(0, p - 1))}
            title={'Previous page'}
            className={pageBtn}
          >
            <ChevronLeft size={16} />
          </button>
          <span className="min-w-[96px] text-center text-[12px] text-white/45">
            {`Page ${page + 1}`}
          </span>
          <button
            type="button"
            disabled={!canNext}
            onClick={() => setPage((p) => p + 1)}
            title={'Next page'}
            className={pageBtn}
          >
            <ChevronRight size={16} />
          </button>
          {q.isFetching && <Loader2 size={14} className="animate-spin text-white/25" />}
        </div>
      )}
    </TabWrapper>
  );
}

/**
 * Поиск треков юзера в нашей базе (`/search/db/tracks?user_urn=...`). Идёт
 * только локально — на SC нет API "tracks этого юзера с подстрочным q=".
 * Рендер совместим с обычным UserTracksTab, чтобы UI не "прыгал" при
 * включении/выключении поиска.
 */
export function UserSearchTracksTab({
  urn,
  aura,
  query,
}: {
  urn: string;
  aura: Aura;
  query: string;
}) {
  const q = useSearchDbTracks(query, urn);
  const ref = useInfiniteScroll(!!q.hasNextPage, !!q.isFetchingNextPage, q.fetchNextPage);
  const renderItem = useCallback(
    (track: (typeof q.tracks)[number], i: number) => (
      <ThemedTrackRow track={track} index={i} queue={q.tracks} aura={aura} />
    ),
    [aura, q.tracks],
  );
  return (
    <TabWrapper
      isLoading={q.isLoading}
      isEmpty={q.tracks.length === 0}
      emptyText={"No matches in this user's content"}
    >
      <VirtualList
        items={q.tracks}
        rowHeight={64}
        overscan={8}
        className="flex flex-col gap-1"
        getItemKey={(t) => t.urn}
        renderItem={renderItem}
      />
      <div ref={ref} className="h-16 flex items-center justify-center">
        {q.isFetchingNextPage && <Loader2 size={20} className="text-white/20 animate-spin" />}
      </div>
    </TabWrapper>
  );
}

/**
 * Поиск плейлистов юзера в нашей базе. Та же логика, что и Tracks-вариант.
 */
export function UserSearchPlaylistsTab({ urn, query }: { urn: string; query: string }) {
  const q = useSearchDbPlaylists(query, urn);
  const ref = useInfiniteScroll(!!q.hasNextPage, !!q.isFetchingNextPage, q.fetchNextPage);
  const renderItem = useCallback(
    (p: (typeof q.playlists)[number]) => <PlaylistCard playlist={p} showPlayback />,
    [],
  );
  return (
    <TabWrapper
      isLoading={q.isLoading}
      isEmpty={q.playlists.length === 0}
      emptyText={"No matches in this user's content"}
    >
      <VirtualGrid
        items={q.playlists}
        itemHeight={250}
        minColumnWidth={140}
        gap={16}
        overscan={3}
        getItemKey={(p, i) => `${p.urn}-${i}`}
        renderItem={renderItem}
      />
      <div ref={ref} className="h-16 flex items-center justify-center">
        {q.isFetchingNextPage && <Loader2 size={20} className="text-white/20 animate-spin" />}
      </div>
    </TabWrapper>
  );
}

export function UserConnectionsTab({
  urn,
  mode,
}: {
  urn: string;
  mode: 'followers' | 'followings';
}) {
  const nav = useNavigate();
  const followers = useUserFollowers(mode === 'followers' ? urn : undefined);
  const followings = useUserFollowings(mode === 'followings' ? urn : undefined);
  const q = mode === 'followers' ? followers : followings;
  const ref = useInfiniteScroll(!!q.hasNextPage, !!q.isFetchingNextPage, q.fetchNextPage);

  const renderItem = useCallback(
    (user: (typeof q.users)[number]) => (
      <button
        type="button"
        onClick={() => nav(`/user/${encodeURIComponent(user.urn)}`)}
        className="group flex h-full w-full flex-col items-center gap-2 p-3 cursor-pointer"
      >
        <div className="h-16 w-16 overflow-hidden rounded-full ring-1 ring-white/10 transition-colors group-hover:ring-white/30">
          <Avatar src={user.avatar_url} alt={user.username} size={64} />
        </div>
        <div className="min-w-0 w-full text-center">
          <p className="truncate text-[13px] font-medium text-white/85 group-hover:text-white">
            {user.username}
          </p>
          {user.followers_count != null && (
            <p className="mt-0.5 text-[10.5px] tabular-nums text-white/35">
              {fc(user.followers_count)} {'Followers'}
            </p>
          )}
        </div>
      </button>
    ),
    [nav],
  );

  const emptyText = mode === 'followers' ? 'No followers found.' : 'No followings found.';

  return (
    <TabWrapper isLoading={q.isLoading} isEmpty={q.users.length === 0} emptyText={emptyText}>
      <VirtualGrid
        items={q.users}
        itemHeight={132}
        minColumnWidth={150}
        gap={12}
        overscan={3}
        getItemKey={(u) => u.urn}
        renderItem={renderItem}
      />
      <div ref={ref} className="h-16 flex items-center justify-center">
        {q.isFetchingNextPage && <Loader2 size={20} className="text-white/20 animate-spin" />}
      </div>
    </TabWrapper>
  );
}
