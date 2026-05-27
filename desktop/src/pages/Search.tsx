import React, { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { AlbumGridCard } from '../components/discover/AlbumGridCard';
import { ArtistGridCard } from '../components/discover/ArtistGridCard';
import { AddToPlaylistDialog } from '../components/music/AddToPlaylistDialog';
import { LikeButton } from '../components/music/LikeButton';
import { PlaylistCard } from '../components/music/PlaylistCard';
import { TrackTitleArtist } from '../components/music/TrackTitleArtist';
import { VirtualList } from '../components/ui/VirtualList';
import { api } from '../lib/api';
import { preloadTrack } from '../lib/audio';
import { DEFAULT_AURA } from '../lib/aura';
import { art, dur, fc } from '../lib/formatters';
import {
  type SCUser,
  useInfiniteScroll,
  useSearchDbAlbums,
  useSearchDbArtists,
  useSearchDbPlaylists,
  useSearchDbTracks,
  useSearchDbUsers,
  useSearchPlaylists,
  useSearchTracks,
  useSearchUsers,
} from '../lib/hooks';
import {
  Clock,
  Database,
  ExternalLink,
  headphones11,
  heart11,
  ListPlus,
  Loader2,
  musicIcon20,
  Pause,
  Play,
  Search as SearchIcon,
  Trash2,
  Users,
  X,
} from '../lib/icons';
import { useTrackPlay } from '../lib/useTrackPlay';
import type { Track } from '../stores/player';
import { useSearchHistoryStore } from '../stores/searchHistory';
import { useSearchPrefsStore } from '../stores/searchPrefs';

/* ── Components ───────────────────────────────────────────── */

const TrackRow = React.memo(
  function TrackRow({ track, queue }: { track: Track; queue: Track[] }) {
    const { t } = useTranslation();
    const { isThis, isThisPlaying, togglePlay } = useTrackPlay(track, queue);
    const cover = art(track.artwork_url, 't200x200');

    return (
      <div
        className={`group flex items-center gap-4 px-4 py-3 rounded-2xl transition-all duration-300 ease-[var(--ease-apple)] ${
          isThis
            ? 'bg-accent/[0.06] ring-1 ring-accent/20 shadow-[inset_0_0_20px_rgba(255,85,0,0.05)]'
            : 'hover:bg-white/[0.04]'
        }`}
        onMouseEnter={() => preloadTrack(track.urn)}
      >
        <div
          className="w-10 h-10 flex items-center justify-center shrink-0 cursor-pointer"
          onClick={togglePlay}
        >
          {isThisPlaying ? (
            <div className="w-9 h-9 rounded-full bg-accent text-accent-contrast flex items-center justify-center shadow-[0_0_15px_var(--color-accent-glow)] scale-100 animate-fade-in-up">
              <Pause size={16} fill="currentColor" strokeWidth={0} />
            </div>
          ) : (
            <div className="w-9 h-9 rounded-full bg-white/[0.06] group-hover:bg-white/10 flex items-center justify-center transition-all">
              <Play
                size={16}
                fill="white"
                strokeWidth={0}
                className="ml-0.5 opacity-60 group-hover:opacity-100"
              />
            </div>
          )}
        </div>

        <div className="relative w-12 h-12 rounded-xl overflow-hidden shrink-0 ring-1 ring-white/[0.08] shadow-md">
          {cover ? (
            <img src={cover} alt="" className="w-full h-full object-cover" decoding="async" />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.05] to-transparent">
              {musicIcon20}
            </div>
          )}
        </div>

        <TrackTitleArtist
          track={track}
          highlight={isThis}
          size="md"
          className="flex flex-col justify-center"
        />

        <div className="hidden md:flex items-center gap-4 shrink-0 pr-4">
          {track.playback_count != null && (
            <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-16">
              {headphones11}
              {fc(track.playback_count)}
            </span>
          )}
          <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-14">
            {heart11}
            {fc(track.favoritings_count ?? track.likes_count)}
          </span>
        </div>

        <div className="flex items-center gap-0.5 shrink-0">
          <LikeButton track={track} />
          <AddToPlaylistDialog trackUrns={[track.urn]}>
            <button
              type="button"
              className="cursor-pointer w-8 h-8 rounded-lg flex items-center justify-center text-white/20 hover:text-white/50 opacity-0 group-hover:opacity-100 transition-all duration-200"
              title={t('playlist.addToPlaylist')}
            >
              <ListPlus size={14} />
            </button>
          </AddToPlaylistDialog>
        </div>

        <span className="text-[12px] text-white/30 tabular-nums font-medium shrink-0 w-12 text-right">
          {dur(track.duration)}
        </span>
      </div>
    );
  },
  (prev, next) =>
    prev.track.urn === next.track.urn && prev.track.user_favorite === next.track.user_favorite,
);

const UserCard = React.memo(({ user }: { user: SCUser }) => {
  const navigate = useNavigate();
  const avatar = art(user.avatar_url, 't300x300');

  return (
    <div
      className="group flex flex-col items-center gap-4 p-5 rounded-3xl bg-white/[0.02] hover:bg-white/[0.04] border border-white/[0.04] hover:border-white/[0.08] transition-all duration-300 cursor-pointer"
      onClick={() => navigate(`/user/${encodeURIComponent(user.urn)}`)}
    >
      <div className="relative w-24 h-24 rounded-full shadow-xl overflow-hidden ring-2 ring-white/[0.05] group-hover:ring-white/[0.15] group-hover:scale-105 transition-all duration-500">
        {avatar ? (
          <img
            src={avatar}
            alt={user.username}
            className="w-full h-full object-cover"
            decoding="async"
          />
        ) : (
          <div className="w-full h-full bg-white/5 flex items-center justify-center">
            <Users size={32} className="text-white/20" />
          </div>
        )}
      </div>

      <div className="text-center w-full">
        <p className="text-[15px] font-bold text-white/90 truncate group-hover:text-white transition-colors">
          {user.username}
        </p>
        <div className="flex items-center justify-center gap-3 mt-2 text-[11px] text-white/30 font-medium">
          <span className="uppercase tracking-wider flex items-center gap-1">
            <Users size={10} />
            {fc(user.followers_count)}
          </span>
        </div>
      </div>
    </div>
  );
});

/* ── URL Detection ───────────────────────────────────────── */

const SC_URL_RE = /^https?:\/\/(www\.|m\.|on\.)?soundcloud\.com\/.+/i;

function isSoundCloudUrl(input: string): boolean {
  return SC_URL_RE.test(input.trim());
}

/* ── Resolve Card ────────────────────────────────────────── */

function ResolveCard({ url, onDone }: { url: string; onDone: () => void }) {
  const navigate = useNavigate();
  const [state, setState] = useState<'loading' | 'error' | 'success'>('loading');
  const [errorMsg, setErrorMsg] = useState('');

  useEffect(() => {
    let cancelled = false;
    setState('loading');

    api<{ kind: string; urn: string }>(`/resolve?url=${encodeURIComponent(url.trim())}`)
      .then((res) => {
        if (cancelled) return;
        setState('success');
        const kind = res.kind;
        const urn = res.urn;
        if (kind === 'track') {
          navigate(`/track/${encodeURIComponent(urn)}`);
        } else if (kind === 'playlist' || kind === 'system-playlist') {
          navigate(`/playlist/${encodeURIComponent(urn)}`);
        } else if (kind === 'user') {
          navigate(`/user/${encodeURIComponent(urn)}`);
        } else {
          setErrorMsg(`Unknown resource: ${kind}`);
          setState('error');
        }
        onDone();
      })
      .catch((e) => {
        if (cancelled) return;
        setErrorMsg(e?.body ? 'Link not found' : 'Failed to resolve');
        setState('error');
      });

    return () => {
      cancelled = true;
    };
  }, [url, navigate, onDone]);

  return (
    <div className="max-w-lg mx-auto mt-12 animate-fade-in-up">
      <div className="glass rounded-3xl p-6 border border-white/[0.06]">
        <div className="flex items-center gap-4">
          <div className="w-12 h-12 rounded-2xl bg-accent/10 flex items-center justify-center shrink-0">
            <ExternalLink size={20} className="text-accent" />
          </div>
          <div className="flex-1 min-w-0">
            <p className="text-[13px] font-semibold text-white/80">
              {state === 'loading'
                ? 'Resolving link...'
                : state === 'error'
                  ? 'Could not resolve'
                  : 'Redirecting...'}
            </p>
            <p className="text-[11px] text-white/30 truncate mt-0.5">{url.trim()}</p>
          </div>
          {state === 'loading' && (
            <Loader2 size={20} className="text-accent animate-spin shrink-0" />
          )}
        </div>
        {state === 'error' && <p className="text-[12px] text-red-400/70 mt-3 pl-16">{errorMsg}</p>}
      </div>
    </div>
  );
}

/* ── Search History ──────────────────────────────────────── */

const SearchHistory = React.memo(function SearchHistory({
  onSelect,
}: {
  onSelect: (query: string) => void;
}) {
  const { t } = useTranslation();
  const { queries, removeQuery, clearHistory } = useSearchHistoryStore();

  if (queries.length === 0) return null;

  return (
    <div className="max-w-2xl mx-auto animate-fade-in-up">
      <div className="flex items-center justify-between mb-3 px-1">
        <span className="text-[12px] font-semibold text-white/30 uppercase tracking-wider">
          {t('search.history')}
        </span>
        <button
          type="button"
          onClick={clearHistory}
          className="flex items-center gap-1.5 text-[11px] text-white/25 hover:text-white/60 transition-colors cursor-pointer"
        >
          <Trash2 size={11} />
          {t('search.clearHistory')}
        </button>
      </div>
      <div className="flex flex-col gap-0.5">
        {queries.map((query) => (
          <div
            key={query}
            className="group flex items-center gap-3 px-3 py-2.5 rounded-xl hover:bg-white/[0.04] transition-all duration-200 cursor-pointer"
            onClick={() => onSelect(query)}
          >
            <Clock size={13} className="text-white/20 shrink-0" />
            <span className="flex-1 text-[13px] text-white/60 group-hover:text-white/90 transition-colors truncate">
              {query}
            </span>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                removeQuery(query);
              }}
              className="opacity-0 group-hover:opacity-100 w-5 h-5 flex items-center justify-center text-white/20 hover:text-white/60 transition-all cursor-pointer shrink-0"
            >
              <X size={11} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
});

/* ── Isolated Search Results ──────────────────────────────── */

/**
 * `db` ↔ `sc` различаются только тем, какой хук дергают. Карточки и
 * виртуализация одинаковые — фронт-shape единый (бэк делает project_to_sc_shape).
 */
type SearchSource = 'db' | 'sc';

const SearchTracksTab = React.memo(function SearchTracksTab({
  query,
  source,
}: {
  query: string;
  source: SearchSource;
}) {
  const { t } = useTranslation();
  // Один из двух хуков активен (enabled=!!q). Switch по source — статичная
  // ветка, для query-cache это просто разные queryKey.
  const dbQuery = useSearchDbTracks(source === 'db' ? query : '');
  const scQuery = useSearchTracks(source === 'sc' ? query : '');
  const active = source === 'db' ? dbQuery : scQuery;
  const sentinelRef = useInfiniteScroll(
    !!active.hasNextPage,
    !!active.isFetchingNextPage,
    active.fetchNextPage,
  );
  const tracks = active.tracks;

  return (
    <div className="min-h-[400px]">
      {active.isLoading ? (
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
        </div>
      ) : tracks.length === 0 ? (
        <SearchNoResults source={source} />
      ) : (
        <VirtualList
          items={tracks}
          rowHeight={68}
          overscan={8}
          className="flex flex-col gap-1"
          disabled={tracks.length < 40}
          getItemKey={(track) => track.urn}
          renderItem={(track) => <TrackRow track={track} queue={tracks} />}
        />
      )}
      <div ref={sentinelRef} className="h-20 flex items-center justify-center mt-6">
        {active.isFetchingNextPage && <Loader2 size={24} className="text-white/20 animate-spin" />}
      </div>
      <SearchNoResultsHintFooter
        source={source}
        showHint={!active.isLoading && tracks.length > 0 && !active.hasNextPage}
      />
      {/* keep t available for noResults strings inside SearchNoResults */}
      <span className="hidden">{t('search.noResults')}</span>
    </div>
  );
});

const SearchPlaylistsTab = React.memo(function SearchPlaylistsTab({
  query,
  source,
}: {
  query: string;
  source: SearchSource;
}) {
  const dbQuery = useSearchDbPlaylists(source === 'db' ? query : '');
  const scQuery = useSearchPlaylists(source === 'sc' ? query : '');
  const active = source === 'db' ? dbQuery : scQuery;
  const sentinelRef = useInfiniteScroll(
    !!active.hasNextPage,
    !!active.isFetchingNextPage,
    active.fetchNextPage,
  );
  const playlists = active.playlists;

  return (
    <div className="min-h-[400px]">
      {active.isLoading ? (
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
        </div>
      ) : playlists.length === 0 ? (
        <SearchNoResults source={source} />
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-6">
          {playlists.map((p, i) => (
            <PlaylistCard key={`${p.urn}-${i}`} playlist={p} />
          ))}
        </div>
      )}
      <div ref={sentinelRef} className="h-20 flex items-center justify-center mt-6">
        {active.isFetchingNextPage && <Loader2 size={24} className="text-white/20 animate-spin" />}
      </div>
      <SearchNoResultsHintFooter
        source={source}
        showHint={!active.isLoading && playlists.length > 0 && !active.hasNextPage}
      />
    </div>
  );
});

const SearchUsersTab = React.memo(function SearchUsersTab({
  query,
  source,
}: {
  query: string;
  source: SearchSource;
}) {
  const dbQuery = useSearchDbUsers(source === 'db' ? query : '');
  const scQuery = useSearchUsers(source === 'sc' ? query : '');
  const active = source === 'db' ? dbQuery : scQuery;
  const sentinelRef = useInfiniteScroll(
    !!active.hasNextPage,
    !!active.isFetchingNextPage,
    active.fetchNextPage,
  );
  const users = active.users;

  return (
    <div className="min-h-[400px]">
      {active.isLoading ? (
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
        </div>
      ) : users.length === 0 ? (
        <SearchNoResults source={source} />
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
          {users.map((u, i) => (
            <UserCard key={`${u.urn}-${i}`} user={u} />
          ))}
        </div>
      )}
      <div ref={sentinelRef} className="h-20 flex items-center justify-center mt-6">
        {active.isFetchingNextPage && <Loader2 size={24} className="text-white/20 animate-spin" />}
      </div>
      <SearchNoResultsHintFooter
        source={source}
        showHint={!active.isLoading && users.length > 0 && !active.hasNextPage}
      />
    </div>
  );
});

/**
 * Artists/Albums: только SCD-режим имеет данные. На SC такого таба нет.
 * Каллер обязан скрывать таб при source='sc' — но на всякий случай рендерим
 * вежливый плейсхолдер, а не пустоту.
 */
const SearchArtistsTab = React.memo(function SearchArtistsTab({ query }: { query: string }) {
  const artistsQuery = useSearchDbArtists(query);
  const sentinelRef = useInfiniteScroll(
    !!artistsQuery.hasNextPage,
    !!artistsQuery.isFetchingNextPage,
    artistsQuery.fetchNextPage,
  );

  return (
    <div className="min-h-[400px]">
      {artistsQuery.isLoading ? (
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
        </div>
      ) : artistsQuery.artists.length === 0 ? (
        <SearchNoResults source="db" />
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4 items-stretch">
          {artistsQuery.artists.map((a) => (
            <ArtistGridCard key={a.id} artist={a} />
          ))}
        </div>
      )}
      <div ref={sentinelRef} className="h-20 flex items-center justify-center mt-6">
        {artistsQuery.isFetchingNextPage && (
          <Loader2 size={24} className="text-white/20 animate-spin" />
        )}
      </div>
    </div>
  );
});

const SearchAlbumsTab = React.memo(function SearchAlbumsTab({ query }: { query: string }) {
  const albumsQuery = useSearchDbAlbums(query);
  const sentinelRef = useInfiniteScroll(
    !!albumsQuery.hasNextPage,
    !!albumsQuery.isFetchingNextPage,
    albumsQuery.fetchNextPage,
  );

  return (
    <div className="min-h-[400px]">
      {albumsQuery.isLoading ? (
        <div className="flex justify-center py-20">
          <Loader2 size={32} className="animate-spin text-white/20" />
        </div>
      ) : albumsQuery.albums.length === 0 ? (
        <SearchNoResults source="db" />
      ) : (
        <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4 items-stretch">
          {albumsQuery.albums.map((a) => (
            <AlbumGridCard key={a.id} album={a} aura={DEFAULT_AURA} />
          ))}
        </div>
      )}
      <div ref={sentinelRef} className="h-20 flex items-center justify-center mt-6">
        {albumsQuery.isFetchingNextPage && (
          <Loader2 size={24} className="text-white/20 animate-spin" />
        )}
      </div>
    </div>
  );
});

/* ── Source Toggle ───────────────────────────────────────── */

const SourceToggle = React.memo(function SourceToggle({
  source,
  onChange,
}: {
  source: SearchSource;
  onChange: (s: SearchSource) => void;
}) {
  const { t } = useTranslation();
  const opts: ReadonlyArray<{ id: SearchSource; label: string; icon: React.ReactNode }> = [
    { id: 'db', label: t('search.source.db'), icon: <Database size={13} /> },
    { id: 'sc', label: t('search.source.sc'), icon: <SearchIcon size={13} /> },
  ];
  return (
    <div className="flex items-center justify-center gap-1 p-1 bg-white/[0.02] border border-white/[0.05] rounded-2xl w-fit backdrop-blur-2xl shadow-sm mx-auto">
      {opts.map((o) => {
        const active = source === o.id;
        return (
          <button
            key={o.id}
            type="button"
            onClick={() => onChange(o.id)}
            className={`flex items-center gap-1.5 px-3.5 py-1.5 rounded-xl text-[12px] font-semibold transition-all duration-300 ease-[var(--ease-apple)] cursor-pointer ${
              active
                ? 'bg-white/[0.10] text-white shadow border border-white/[0.06]'
                : 'text-white/40 hover:text-white/80 hover:bg-white/[0.04] border border-transparent'
            }`}
            title={o.id === 'db' ? t('search.source.dbHint') : t('search.source.scHint')}
          >
            {o.icon}
            {o.label}
          </button>
        );
      })}
    </div>
  );
});

/* ── Helpers (empty states) ──────────────────────────────── */

function SearchNoResults({ source }: { source: SearchSource }) {
  const { t } = useTranslation();
  return (
    <div className="py-20 text-center">
      <div className="text-white/30 text-[14px] font-medium">{t('search.noResults')}</div>
      {source === 'db' && (
        <div className="text-white/20 text-[11px] mt-2">{t('search.noResultsDbHint')}</div>
      )}
    </div>
  );
}

function SearchNoResultsHintFooter({
  source,
  showHint,
}: {
  source: SearchSource;
  showHint: boolean;
}) {
  const { t } = useTranslation();
  if (!showHint || source !== 'db') return null;
  return (
    <div className="text-center text-white/25 text-[11px] mt-4 mb-2 font-medium">
      {t('search.tryScHint')}
    </div>
  );
}

const SearchEmpty = React.memo(function SearchEmpty() {
  const { t } = useTranslation();
  return (
    <div className="flex flex-col items-center justify-center h-[400px] text-white/20">
      <SearchIcon size={48} className="mb-4 opacity-50" />
      <p className="text-sm font-medium">{t('search.hint')}</p>
    </div>
  );
});

/* ── Search Page ──────────────────────────────────────────── */

type SearchTab = 'tracks' | 'playlists' | 'users' | 'artists' | 'albums';

const TABS_DB: readonly SearchTab[] = ['tracks', 'playlists', 'users', 'artists', 'albums'];
const TABS_SC: readonly SearchTab[] = ['tracks', 'playlists', 'users'];

export const Search = React.memo(() => {
  const { t } = useTranslation();
  const [inputValue, setInputValue] = useState('');
  const [debouncedQuery, setDebouncedQuery] = useState('');
  const [activeTab, setActiveTab] = useState<SearchTab>('tracks');
  const [resolveUrl, setResolveUrl] = useState<string | null>(null);

  const addQuery = useSearchHistoryStore((s) => s.addQuery);
  const source = useSearchPrefsStore((s) => s.source);
  const setSource = useSearchPrefsStore((s) => s.setSource);

  const isUrl = isSoundCloudUrl(inputValue);

  // Debounce logic — skip debounce for URLs
  useEffect(() => {
    if (isUrl) {
      setDebouncedQuery('');
      return;
    }
    setResolveUrl(null);
    const handler = setTimeout(() => {
      const q = inputValue.trim();
      setDebouncedQuery(q);
      if (q) addQuery(q);
    }, 500);
    return () => clearTimeout(handler);
  }, [inputValue, isUrl, addQuery]);

  // Если переключились на SC, а активный таб — Artists/Albums (только-DB),
  // мягко падаем на tracks, чтобы юзер видел релевантные результаты, а не
  // пустоту.
  const visibleTabs: readonly SearchTab[] = source === 'db' ? TABS_DB : TABS_SC;
  useEffect(() => {
    if (!visibleTabs.includes(activeTab)) {
      setActiveTab('tracks');
    }
  }, [visibleTabs, activeTab]);

  // Handle Enter for URL resolve
  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && isUrl) {
      setResolveUrl(inputValue.trim());
    }
  };

  const handlePaste = (e: React.ClipboardEvent) => {
    const pasted = e.clipboardData.getData('text');
    if (isSoundCloudUrl(pasted)) {
      e.preventDefault();
      setInputValue(pasted);
      setResolveUrl(pasted.trim());
    }
  };

  const handleHistorySelect = (query: string) => {
    setInputValue(query);
    setDebouncedQuery(query);
  };

  const tabs = useMemo(
    () =>
      visibleTabs.map((id) => ({
        id,
        label: t(`search.${id}` as const),
      })),
    [visibleTabs, t],
  );

  const historyQueries = useSearchHistoryStore((s) => s.queries);
  const showHistory = !inputValue && !resolveUrl && historyQueries.length > 0;
  const showEmpty = !inputValue && !resolveUrl && historyQueries.length === 0;

  return (
    <div className="p-6 pb-4 space-y-8">
      {/* Search Input */}
      <div className="relative max-w-2xl mx-auto">
        <div className="absolute inset-y-0 left-4 flex items-center pointer-events-none">
          {isUrl ? (
            <ExternalLink size={20} className="text-accent" />
          ) : (
            <SearchIcon size={20} className="text-white/40" />
          )}
        </div>
        <input
          type="text"
          value={inputValue}
          onChange={(e) => setInputValue(e.target.value)}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          placeholder={t('search.placeholder')}
          className={`w-full bg-white/[0.04] hover:bg-white/[0.06] focus:bg-white/[0.08] text-white placeholder:text-white/30 text-[16px] py-4 pl-12 pr-12 rounded-[20px] outline-none border transition-all duration-300 shadow-xl backdrop-blur-md ${
            isUrl
              ? 'border-accent/30 ring-1 ring-accent/20'
              : 'border-white/[0.05] focus:border-accent/30 focus:ring-1 focus:ring-accent/30'
          }`}
          autoFocus
        />
        {inputValue && (
          <button
            onClick={() => {
              setInputValue('');
              setResolveUrl(null);
            }}
            className="absolute inset-y-0 right-4 flex items-center text-white/30 hover:text-white cursor-pointer transition-colors"
          >
            <X size={18} />
          </button>
        )}
        {isUrl && !resolveUrl && (
          <div className="absolute -bottom-7 left-0 text-[11px] text-accent/60 flex items-center gap-1.5">
            <ExternalLink size={10} />
            Press Enter to open link
          </div>
        )}
      </div>

      {/* Source toggle (hidden during URL resolve to reduce visual noise) */}
      {!isUrl && !resolveUrl && <SourceToggle source={source} onChange={setSource} />}

      {/* Tabs */}
      {debouncedQuery && (
        <div className="flex items-center justify-center gap-1.5 p-1.5 bg-white/[0.02] border border-white/[0.05] rounded-2xl w-fit backdrop-blur-2xl shadow-lg mx-auto flex-wrap">
          {tabs.map((tab) => {
            const isActive = activeTab === tab.id;
            return (
              <button
                key={tab.id}
                onClick={() => setActiveTab(tab.id)}
                className={`flex items-center gap-2 px-6 py-2.5 rounded-xl text-[13px] font-semibold transition-all duration-300 ease-[var(--ease-apple)] cursor-pointer ${
                  isActive
                    ? 'bg-white/[0.12] text-white shadow-md border border-white/[0.05]'
                    : 'text-white/40 hover:text-white/80 hover:bg-white/[0.04] border border-transparent'
                }`}
              >
                {tab.label}
              </button>
            );
          })}
        </div>
      )}

      {/* Resolve */}
      {resolveUrl && (
        <ResolveCard
          url={resolveUrl}
          onDone={() => {
            setInputValue('');
            setResolveUrl(null);
          }}
        />
      )}

      {/* History (when input is empty) */}
      {showHistory && <SearchHistory onSelect={handleHistorySelect} />}

      {/* Empty state (when input is empty and no history) */}
      {showEmpty && <SearchEmpty />}

      {/* Results */}
      {!resolveUrl && debouncedQuery && activeTab === 'tracks' && (
        <SearchTracksTab query={debouncedQuery} source={source} />
      )}
      {!resolveUrl && debouncedQuery && activeTab === 'playlists' && (
        <SearchPlaylistsTab query={debouncedQuery} source={source} />
      )}
      {!resolveUrl && debouncedQuery && activeTab === 'users' && (
        <SearchUsersTab query={debouncedQuery} source={source} />
      )}
      {!resolveUrl && debouncedQuery && activeTab === 'artists' && source === 'db' && (
        <SearchArtistsTab query={debouncedQuery} />
      )}
      {!resolveUrl && debouncedQuery && activeTab === 'albums' && source === 'db' && (
        <SearchAlbumsTab query={debouncedQuery} />
      )}
    </div>
  );
});
