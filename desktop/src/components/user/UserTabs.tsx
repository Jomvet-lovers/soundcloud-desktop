import React, { useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import type { Aura } from '../../lib/aura';
import { fc } from '../../lib/formatters';
import {
  useInfiniteScroll,
  useSearchDbPlaylists,
  useSearchDbTracks,
  useUserFollowers,
  useUserFollowings,
  useUserLikedTracks,
  useUserPlaylists,
  useUserPopularTracks,
  useUserTracks,
} from '../../lib/hooks';
import { Loader2, Music } from '../../lib/icons';
import { PlaylistCard } from '../music/PlaylistCard';
import { Avatar } from '../ui/Avatar';
import { VirtualGrid } from '../ui/VirtualGrid';
import { VirtualList } from '../ui/VirtualList';
import { ThemedTrackRow } from './ThemedTrackRow';

interface TabWrapperProps {
  isLoading: boolean;
  isEmpty: boolean;
  emptyText?: string;
  children: React.ReactNode;
}

function TabWrapperImpl({ children, isLoading, isEmpty, emptyText }: TabWrapperProps) {
  const { t } = useTranslation();
  return (
    <div className="min-h-[420px]">
      {isLoading ? (
        <div className="py-24 flex justify-center">
          <Loader2 size={28} className="text-white/20 animate-spin" />
        </div>
      ) : isEmpty ? (
        <div className="py-24 flex flex-col items-center gap-3">
          <Music size={28} className="text-white/15" />
          <p className="text-white/30 text-sm">{emptyText ?? t('common.empty')}</p>
        </div>
      ) : (
        <div className="animate-in fade-in duration-500">{children}</div>
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

export function UserLikesTab({ urn, aura }: { urn: string; aura: Aura }) {
  const q = useUserLikedTracks(urn);
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
  const { t } = useTranslation();
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
      emptyText={t('user.search.empty')}
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
  const { t } = useTranslation();
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
      emptyText={t('user.search.empty')}
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
  const { t } = useTranslation();
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
              {fc(user.followers_count)} {t('user.followers')}
            </p>
          )}
        </div>
      </button>
    ),
    [nav, t],
  );

  const emptyText = mode === 'followers' ? t('user.noFollowers') : t('user.noFollowings');

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
