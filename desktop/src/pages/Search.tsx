import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { Link, useNavigate } from 'react-router-dom';
import { TrackCard } from '../components/music/TrackCard';
import { api } from '../lib/api';
import { art } from '../lib/formatters';
import {
  fetchSystemPlaylistTracks,
  type MixedSelectionItem,
  type PagedResponse,
  useDiscoverMixed,
  useSearchDbAlbums,
  useSearchDbPlaylists,
  useSearchDbTracks,
  useSearchDbUsers,
} from '../lib/hooks';
import { ChevronRight, Loader2, Music, Play } from '../lib/icons';
import { type Track, usePlayerStore } from '../stores/player';
import { useSearchHistoryStore } from '../stores/searchHistory';
import { useSearchPrefsStore } from '../stores/searchPrefs';
import { useSearchQueryStore } from '../stores/searchQuery';

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
  const cover = art(item.artwork_url ?? item.calculated_artwork_url ?? null, 't500x500');
  return (
    <button
      type="button"
      onClick={() => onPlay(item)}
      disabled={busy}
      className="group cursor-pointer text-left disabled:opacity-60"
    >
      <div className="relative aspect-square overflow-hidden rounded-xl bg-white/[0.04]">
        {cover ? (
          <img
            src={cover}
            alt=""
            loading="lazy"
            className="size-full object-cover transition-opacity group-hover:opacity-85"
          />
        ) : (
          <div className="flex size-full items-center justify-center">
            <Music size={22} className="text-white/20" />
          </div>
        )}
        {busy ? (
          <div className="absolute inset-0 flex items-center justify-center bg-black/50">
            <Loader2 size={20} className="animate-spin text-white/80" />
          </div>
        ) : item.kind === 'user' ? null : (
          <span className="absolute bottom-2 right-2 flex size-8 items-center justify-center rounded-full bg-black/65 text-white/85">
            {item.urn?.startsWith('soundcloud:playlists:') ? (
              <ChevronRight size={15} />
            ) : (
              <Play size={13} fill="currentColor" strokeWidth={0} className="ml-px" />
            )}
          </span>
        )}
      </div>
      <p className="mt-2 truncate text-[13px] font-medium text-white/85">
        {item.short_title || item.title}
      </p>
      <p className="truncate text-[11px] text-white/40">
        {item.short_description || item.description || ''}
      </p>
    </button>
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

/** Search — tabs over SoundCloud, with Discover selections when the query is empty. */
export function Search() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const q = useSearchQueryStore((s) => s.q);
  const setQ = useSearchQueryStore((s) => s.setQ);
  const addQuery = useSearchHistoryStore((s) => s.addQuery);
  const [debounced, setDebounced] = useState(q);
  const tab = useSearchPrefsStore((s) => s.tab);
  const setTab = useSearchPrefsStore((s) => s.setTab);
  const [busyUrn, setBusyUrn] = useState<string | null>(null);
  const play = usePlayerStore((s) => s.play);

  useEffect(() => {
    const id = setTimeout(() => setDebounced(q), 300);
    return () => clearTimeout(id);
  }, [q]);

  const query = debounced.trim();
  const tracks = useSearchDbTracks(query);
  const users = useSearchDbUsers(query);
  const playlists = useSearchDbPlaylists(query);
  const albums = useSearchDbAlbums(query);
  const mixed = useDiscoverMixed();

  const tabs = useMemo(
    () =>
      [
        { id: 'tracks' as const, label: t('search.tracks'), count: tracks.tracks.length },
        { id: 'users' as const, label: t('search.users'), count: users.users.length },
        {
          id: 'playlists' as const,
          label: t('search.playlists'),
          count: playlists.playlists.length,
        },
        { id: 'albums' as const, label: t('search.albums'), count: albums.albums.length },
      ] as const,
    [t, tracks.tracks.length, users.users.length, playlists.playlists.length, albums.albums.length],
  );

  const submit = () => {
    if (query) addQuery(query);
  };

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

  const empty =
    query.length > 0 &&
    tracks.tracks.length === 0 &&
    users.users.length === 0 &&
    playlists.playlists.length === 0 &&
    albums.albums.length === 0;

  const selections = mixed.data?.collection ?? [];

  return (
    <div className="px-5 py-6 md:px-8">
      <h1 className="text-[24px] font-semibold tracking-tight text-white/92">
        {t('search.caption')}
      </h1>

      <input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') submit();
        }}
        autoComplete="off"
        spellCheck={false}
        placeholder={t('search.placeholder')}
        className="mt-4 w-full max-w-[520px] rounded-xl border border-white/[0.08] bg-white/[0.05] px-3.5 py-2.5 text-[13px] text-white/85 outline-none placeholder:text-white/25 focus:border-white/20"
      />

      {!query ? (
        <div className="mt-6 flex flex-col gap-8">
          {mixed.isLoading ? (
            <p className="text-[13px] text-white/35">{t('common.loading')}</p>
          ) : selections.length === 0 ? (
            <p className="text-[13px] text-white/35">{t('search.firstTimeTitle')}</p>
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
      ) : (
        <>
          <div className="mt-4 flex flex-wrap items-center gap-2">
            {tabs.map((item) => (
              <button
                key={item.id}
                type="button"
                onClick={() => setTab(item.id)}
                className={`rounded-lg px-3 py-1.5 text-[12px] font-medium transition-colors ${
                  tab === item.id
                    ? 'bg-white/[0.1] text-white/90'
                    : 'text-white/45 hover:bg-white/[0.05] hover:text-white/70'
                }`}
              >
                {item.label}
                {item.count > 0 && <span className="ml-1.5 text-white/35">{item.count}</span>}
              </button>
            ))}
          </div>

          <div className="mt-5">
            {empty ? (
              <p className="text-[13px] text-white/35">{t('search.noResults')}</p>
            ) : tab === 'tracks' ? (
              <div className="grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8">
                {tracks.tracks.map((track) => (
                  <TrackCard key={track.urn} track={track} queue={tracks.tracks} />
                ))}
              </div>
            ) : tab === 'users' ? (
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
            ) : tab === 'playlists' ? (
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
            ) : (
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
            )}
          </div>
        </>
      )}
    </div>
  );
}
