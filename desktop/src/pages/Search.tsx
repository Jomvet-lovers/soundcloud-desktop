import {useEffect, useMemo, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {Link} from 'react-router-dom';
import {TrackCard} from '../components/music/TrackCard';
import {art} from '../lib/formatters';
import {
  useSearchDbAlbums,
  useSearchDbPlaylists,
  useSearchDbTracks,
  useSearchDbUsers,
} from '../lib/hooks';
import {useSearchHistoryStore} from '../stores/searchHistory';
import {useSearchQueryStore} from '../stores/searchQuery';

type Tab = 'tracks' | 'users' | 'playlists' | 'albums';

/** Search — plain tabs over the SoundCloud-backed /search/db endpoints. */
export function Search() {
  const { t } = useTranslation();
  const q = useSearchQueryStore((s) => s.q);
  const setQ = useSearchQueryStore((s) => s.setQ);
  const addQuery = useSearchHistoryStore((s) => s.addQuery);
  const [debounced, setDebounced] = useState(q);
  const [tab, setTab] = useState<Tab>('tracks');

  useEffect(() => {
    const id = setTimeout(() => setDebounced(q), 300);
    return () => clearTimeout(id);
  }, [q]);

  const query = debounced.trim();
  const tracks = useSearchDbTracks(query);
  const users = useSearchDbUsers(query);
  const playlists = useSearchDbPlaylists(query);
  const albums = useSearchDbAlbums(query);

  const tabs = useMemo(
    () =>
      [
        {id: 'tracks' as const, label: t('search.tracks'), count: tracks.tracks.length},
        {id: 'users' as const, label: t('search.users'), count: users.users.length},
        {id: 'playlists' as const, label: t('search.playlists'), count: playlists.playlists.length},
        {id: 'albums' as const, label: t('search.albums'), count: albums.albums.length},
      ] as const,
    [t, tracks.tracks.length, users.users.length, playlists.playlists.length, albums.albums.length],
  );

  const submit = () => {
    if (query) addQuery(query);
  };

  const empty =
    query.length > 0 &&
    tracks.tracks.length === 0 &&
    users.users.length === 0 &&
    playlists.playlists.length === 0 &&
    albums.albums.length === 0;

  return (
    <div className="px-5 py-6 md:px-8">
      <h1 className="text-[24px] font-semibold tracking-tight text-white/92">{t('search.caption')}</h1>

      <input
        value={q}
        onChange={(e) => setQ(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') submit();
        }}
        placeholder={t('search.placeholder')}
        className="mt-4 w-full max-w-[520px] rounded-xl border border-white/[0.08] bg-white/[0.05] px-3.5 py-2.5 text-[13px] text-white/85 outline-none placeholder:text-white/25 focus:border-white/20"
      />

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
        {!query ? (
          <p className="text-[13px] text-white/35">{t('search.firstTimeTitle')}</p>
        ) : empty ? (
          <p className="text-[13px] text-white/35">{t('search.noResults')}</p>
        ) : tab === 'tracks' ? (
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4 xl:grid-cols-5">
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
                <img
                  src={art(user.avatar_url, 't120x120') ?? ''}
                  alt=""
                  className="size-9 rounded-full bg-white/[0.06] object-cover"
                />
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
                <img
                  src={art(playlist.artwork_url, 't120x120') ?? ''}
                  alt=""
                  className="size-9 rounded-lg bg-white/[0.06] object-cover"
                />
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
                <img
                  src={art(album.cover_url ?? null, 't120x120') ?? ''}
                  alt=""
                  className="size-9 rounded-lg bg-white/[0.06] object-cover"
                />
                <span className="text-[13px] text-white/85">{album.title}</span>
              </Link>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
