import { type MouseEvent, useEffect, useMemo, useState } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import { TrackCard } from '../components/music/TrackCard';
import { GenreGrid } from '../components/search/GenreGrid';
import { Pager } from '../components/ui/Pager';
import { api } from '../lib/api';
import { art } from '../lib/formatters';
import {
  fetchSystemPlaylistTracks,
  type MixedSelectionItem,
  type PagedResponse,
  useDiscoverMixed,
  useSearchDbAlbumsPage,
  useSearchDbPlaylistsPage,
  useSearchDbTracksPage,
  useSearchDbUsersPage,
} from '../lib/hooks';
import { LinkIcon, Loader2, Music, playBlack20 } from '../lib/icons';
import { withViewTransition } from '../lib/view-transition';
import { type Track, usePlayerStore } from '../stores/player';
import { useSearchPrefsStore } from '../stores/searchPrefs';
import { useSearchQueryStore } from '../stores/searchQuery';

/** Debounced view of the global search query — the header field writes it. */
export function useDebouncedSearchQuery(): string {
  const q = useSearchQueryStore((s) => s.q);
  const [debounced, setDebounced] = useState(q);
  useEffect(() => {
    const id = setTimeout(() => setDebounced(q), 300);
    return () => clearTimeout(id);
  }, [q]);
  return debounced.trim();
}

/** Where a Discover item's title should lead (null → not a page we host). */
function itemPagePath(item: MixedSelectionItem): string | null {
  const urn = item.urn ?? '';
  if (urn.startsWith('soundcloud:tracks:')) return `/track/${encodeURIComponent(urn)}`;
  if (urn.startsWith('soundcloud:users:')) return `/user/${encodeURIComponent(urn)}`;
  if (urn.startsWith('soundcloud:playlists:') || urn.startsWith('soundcloud:system-playlists:')) {
    return `/playlist/${encodeURIComponent(urn)}`;
  }
  return null;
}

/** Discover card for a SoundCloud mixed-selection item (mix or artist station). */
function DiscoverCard({
  item,
  busy,
  onPlay,
}: {
  item: MixedSelectionItem;
  busy: boolean;
  onPlay: (item: MixedSelectionItem) => void;
}) {
  const navigate = useNavigate();
  const cover = art(item.artwork_url ?? item.calculated_artwork_url ?? null, 't500x500');
  const pagePath = itemPagePath(item);

  const copyLink = async (e: MouseEvent) => {
    e.stopPropagation();
    const url = item.permalink_url;
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      toast.success('Copied!');
    } catch {
      toast.error('Something went wrong');
    }
  };

  return (
    <div className="group min-w-0">
      <div
        className={`relative aspect-square overflow-hidden rounded-xl bg-white/[0.04] cursor-pointer ${
          busy ? 'opacity-60 pointer-events-none' : ''
        }`}
        onClick={() => !busy && onPlay(item)}
      >
        {cover ? (
          <img
            src={cover}
            alt=""
            loading="lazy"
            className="size-full object-cover transition-transform duration-500 ease-[var(--ease-apple)] group-hover:scale-[1.04]"
          />
        ) : (
          <div className="flex size-full items-center justify-center">
            <Music size={22} className="text-white/20" />
          </div>
        )}
        {/* Hover: dim + centred play, same as the HOME track cards. */}
        {!busy && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/0 opacity-0 transition-all duration-300 group-hover:bg-black/30 group-hover:opacity-100">
            <div className="flex w-10 h-10 items-center justify-center rounded-full bg-white/90 scale-75 shadow-xl transition-all duration-300 ease-[var(--ease-apple)] group-hover:scale-100">
              {playBlack20}
            </div>
          </div>
        )}
        {busy && (
          <div className="absolute inset-0 flex items-center justify-center bg-black/50">
            <Loader2 size={20} className="animate-spin text-white/80" />
          </div>
        )}
        {/* Copy link — top right, on hover (HOME-style). */}
        <div className="absolute top-2 right-2 flex items-center gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity duration-200">
          <button
            type="button"
            onClick={copyLink}
            className="cursor-pointer w-6 h-6 rounded-full bg-black/50 flex items-center justify-center text-white/80 hover:text-white hover:bg-black/70 transition-all duration-200"
            title={'Copy link'}
          >
            <LinkIcon size={12} />
          </button>
        </div>
      </div>
      {pagePath ? (
        <button
          type="button"
          onClick={() => navigate(pagePath)}
          className="mt-2 block w-full truncate text-left text-[13px] font-medium text-white/85 hover:text-white transition-colors cursor-pointer"
        >
          {item.short_title || item.title}
        </button>
      ) : (
        <p className="mt-2 truncate text-[13px] font-medium text-white/85">
          {item.short_title || item.title}
        </p>
      )}
      <p className="truncate text-[11px] text-white/40">
        {item.short_description || item.description || ''}
      </p>
    </div>
  );
}

/** Small list thumbnail with a music-note placeholder when artwork is missing. */
function RowArt({ src, rounded }: { src: string | null; rounded: 'full' | 'lg' }) {
  const cls = rounded === 'full' ? 'rounded-full' : 'rounded-lg';
  return src ? (
    <img src={src} alt="" className={`size-9 ${cls} bg-white/[0.06] object-cover`} />
  ) : (
    <div className={`flex size-9 shrink-0 items-center justify-center ${cls} bg-white/[0.06]`}>
      <Music size={14} className="text-white/25" />
    </div>
  );
}

/** De-duplicates selection items (SC repeats the same artist in "Recently Played"). */
function dedupeItems(items: MixedSelectionItem[]): MixedSelectionItem[] {
  const seen = new Set<string>();
  const out: MixedSelectionItem[] = [];
  for (const item of items) {
    const key = item.urn ?? `${item.kind}:${item.title}:${item.artwork_url ?? ''}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(item);
  }
  return out;
}

/** Discover selections — the "Made for you" / station rows under the Home shelf. */
export function DiscoverSections() {
  const navigate = useNavigate();
  const [busyUrn, setBusyUrn] = useState<string | null>(null);
  const play = usePlayerStore((s) => s.play);
  const mixed = useDiscoverMixed();
  const selections = mixed.data?.collection ?? [];

  const startDiscoverItem = async (item: MixedSelectionItem) => {
    if (busyUrn) return;
    const urn = item.urn;
    if (!urn) return;

    // Artists ("Recently Played", "New Music From …") open their profile.
    if (item.kind === 'user' || urn.startsWith('soundcloud:users:')) {
      navigate(`/user/${encodeURIComponent(urn)}`);
      return;
    }

    // Artist / track stations -> local radio (SC's station playback API is gone).
    const stationUser = urn.match(/artist-stations:(\d+)/)?.[1];
    const stationTrack = urn.match(/track-stations:(\d+)/)?.[1];
    if (stationUser || stationTrack || item.playlist_type === 'ARTIST_STATION') {
      if (!stationUser && !stationTrack) return;
      setBusyUrn(urn);
      try {
        let list: Track[] = [];
        if (stationUser) {
          const res = await api<PagedResponse<Track>>(
            `/users/${stationUser}/tracks?limit=50&offset=0`,
          );
          list = (res.collection ?? []).filter((track) => track?.urn);
          for (let i = list.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [list[i], list[j]] = [list[j], list[i]];
          }
        } else if (stationTrack) {
          const res = await api<PagedResponse<Track>>(
            `/tracks/${stationTrack}/related?limit=30&offset=0`,
          );
          list = (res.collection ?? []).filter((track) => track?.urn);
        }
        if (list.length > 0) play(list[0], list);
      } catch {
        // Card just stops spinning; keep the page usable.
      } finally {
        setBusyUrn(null);
      }
      return;
    }

    // SoundCloud system mixes (Your Mix, Daily Drops, Trending, …) play in place.
    if (urn.startsWith('soundcloud:system-playlists:')) {
      setBusyUrn(urn);
      try {
        const list = await fetchSystemPlaylistTracks(urn);
        if (list.length > 0) play(list[0], list);
      } catch {
        // ignore
      } finally {
        setBusyUrn(null);
      }
      return;
    }

    // Regular SoundCloud playlists/albums open their page.
    if (urn.startsWith('soundcloud:playlists:')) {
      navigate(`/playlist/${encodeURIComponent(urn)}`);
    }
  };

  return (
    <div className="flex flex-col gap-8">
      {mixed.isLoading ? (
        <p className="text-[13px] text-white/35">{'Loading...'}</p>
      ) : selections.length === 0 ? (
        <p className="text-[13px] text-white/35">{'Start exploring'}</p>
      ) : (
        selections.map((sel) => {
          const items = dedupeItems(sel.items?.collection ?? []);
          if (items.length === 0) return null;
          return (
            <section key={sel.urn}>
              <h2 className="text-[16px] font-semibold tracking-tight text-white/90">
                {sel.title}
              </h2>
              <div className="mt-3 grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8">
                {items.slice(0, 10).map((item, idx) => (
                  <DiscoverCard
                    key={item.urn ?? idx}
                    item={item}
                    busy={busyUrn === item.urn}
                    onPlay={startDiscoverItem}
                  />
                ))}
              </div>
            </section>
          );
        })
      )}
    </div>
  );
}

/** SoundCloud search results — tabs, sorting and numbered pagination. */
function SearchResults() {
  const query = useDebouncedSearchQuery();
  const tab = useSearchPrefsStore((s) => s.tab);
  const setTab = useSearchPrefsStore((s) => s.setTab);
  const sort = useSearchPrefsStore((s) => s.sort);
  const setSort = useSearchPrefsStore((s) => s.setSort);

  // Numbered pagination: the page resets whenever the query / tab / sort
  // changes, and only the visible tab requests deeper pages (the other tabs
  // keep their first-page counts).
  const pagerKey = `${query}\u{1}${tab}\u{1}${sort}`;
  const [pager, setPager] = useState<{ key: string; page: number }>({ key: '', page: 0 });
  const page = pager.key === pagerKey ? pager.page : 0;
  const changePage = (next: number) => {
    setPager({ key: pagerKey, page: Math.max(0, next) });
    (document.querySelector('main') as HTMLElement | null)?.scrollTo({
      top: 0,
      behavior: 'smooth',
    });
  };

  const tracks = useSearchDbTracksPage(query, tab === 'tracks' ? page : 0, undefined, sort);
  const users = useSearchDbUsersPage(query, tab === 'users' ? page : 0);
  const playlists = useSearchDbPlaylistsPage(query, tab === 'playlists' ? page : 0);
  const albums = useSearchDbAlbumsPage(query, tab === 'albums' ? page : 0);

  const tabs = useMemo(
    () =>
      [
        { id: 'tracks' as const, label: 'Tracks', count: tracks.tracks.length },
        { id: 'users' as const, label: 'Users', count: users.users.length },
        {
          id: 'playlists' as const,
          label: 'Playlists',
          count: playlists.playlists.length,
        },
        { id: 'albums' as const, label: 'Albums', count: albums.albums.length },
      ] as const,
    [tracks.tracks.length, users.users.length, playlists.playlists.length, albums.albums.length],
  );

  const empty =
    query.length > 0 &&
    tracks.tracks.length === 0 &&
    users.users.length === 0 &&
    playlists.playlists.length === 0 &&
    albums.albums.length === 0;

  if (!query) return null;

  return (
    <>
      <div className="flex flex-wrap items-center gap-2">
        {tabs.map((item) => {
          const on = tab === item.id;
          return (
            <button
              key={item.id}
              type="button"
              onClick={() => withViewTransition(() => setTab(item.id))}
              className={`relative rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors ${
                on ? 'text-white/90' : 'text-white/45 hover:text-white/70'
              }`}
            >
              <span
                aria-hidden
                className={`pointer-events-none absolute inset-0 rounded-lg bg-white/[0.1] transition-opacity duration-200 ease-out ${
                  on ? 'vt-tab-pill opacity-100' : 'opacity-0'
                }`}
              />
              <span className="relative">
                {item.label}
                {item.count > 0 && <span className="ml-1.5 text-white/35">{item.count}</span>}
              </span>
            </button>
          );
        })}
        {tab === 'tracks' && (
          <div className="ml-auto flex items-center gap-0.5">
            {[
              { id: 'relevance' as const, label: 'Relevance' },
              { id: 'plays' as const, label: 'Plays' },
              { id: 'newest' as const, label: 'Newest' },
              { id: 'likes' as const, label: 'Likes' },
            ].map((opt) => {
              const on = sort === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  onClick={() => withViewTransition(() => setSort(opt.id))}
                  className={`relative rounded-lg px-2.5 py-1.5 text-[11px] font-medium transition-colors ${
                    on ? 'text-white/90' : 'text-white/40 hover:text-white/70'
                  }`}
                >
                  <span
                    aria-hidden
                    className={`pointer-events-none absolute inset-0 rounded-lg bg-white/[0.1] transition-opacity duration-200 ease-out ${
                      on ? 'vt-sort-pill opacity-100' : 'opacity-0'
                    }`}
                  />
                  <span className="relative">{opt.label}</span>
                </button>
              );
            })}
          </div>
        )}
      </div>

      <div key={tab} className="mt-5 animate-soft-in">
        {empty ? (
          <p className="text-[13px] text-white/35">{'No results found'}</p>
        ) : tab === 'tracks' ? (
          <>
            <div className="grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8">
              {tracks.tracks.map((track) => (
                <TrackCard key={track.urn} track={track} queue={tracks.tracks} />
              ))}
            </div>
            <Pager
              page={page}
              hasMore={tracks.hasMore}
              isFetching={tracks.isFetching}
              onPage={changePage}
            />
          </>
        ) : tab === 'users' ? (
          <>
            <div className="flex flex-col gap-1">
              {users.users.map((user) => (
                <Link
                  key={user.urn}
                  to={`/user/${encodeURIComponent(user.urn)}`}
                  className="flex items-center gap-3 rounded-xl px-3 py-2 hover:bg-white/[0.05]"
                >
                  <RowArt src={art(user.avatar_url, 't120x120')} rounded="full" />
                  <span className="text-[13px] text-white/85">{user.username}</span>
                </Link>
              ))}
            </div>
            <Pager
              page={page}
              hasMore={users.hasMore}
              isFetching={users.isFetching}
              onPage={changePage}
            />
          </>
        ) : tab === 'playlists' ? (
          <>
            <div className="flex flex-col gap-1">
              {playlists.playlists.map((playlist) => (
                <Link
                  key={playlist.urn}
                  to={`/playlist/${encodeURIComponent(playlist.urn)}`}
                  className="flex items-center gap-3 rounded-xl px-3 py-2 hover:bg-white/[0.05]"
                >
                  <RowArt src={art(playlist.artwork_url, 't120x120')} rounded="lg" />
                  <span className="text-[13px] text-white/85">{playlist.title}</span>
                </Link>
              ))}
            </div>
            <Pager
              page={page}
              hasMore={playlists.hasMore}
              isFetching={playlists.isFetching}
              onPage={changePage}
            />
          </>
        ) : (
          <>
            <div className="flex flex-col gap-1">
              {albums.albums.map((album) => (
                <Link
                  key={album.id}
                  to={`/album/${encodeURIComponent(album.id)}`}
                  className="flex items-center gap-3 rounded-xl px-3 py-2 hover:bg-white/[0.05]"
                >
                  <RowArt src={art(album.cover_url ?? null, 't120x120')} rounded="lg" />
                  <span className="text-[13px] text-white/85">{album.title}</span>
                </Link>
              ))}
            </div>
            <Pager
              page={page}
              hasMore={albums.hasMore}
              isFetching={albums.isFetching}
              onPage={changePage}
            />
          </>
        )}
      </div>
    </>
  );
}

/** Search — the dedicated results tab. An empty query shows the genre wall. */
export function Search() {
  const query = useDebouncedSearchQuery();
  return (
    <div className="px-5 py-6 md:px-8">
      {query ? (
        <SearchResults />
      ) : (
        <>
          <h2 className="mb-4 text-[16px] font-semibold tracking-tight text-white/90">
            {'Browse all genres'}
          </h2>
          <GenreGrid />
        </>
      )}
    </div>
  );
}
