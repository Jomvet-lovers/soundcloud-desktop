import React, { useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { useShallow } from 'zustand/shallow';
import { SoundWaveBlock, SoundWaveLockOverlay } from '../components/music/soundwave';
import { TrackCard } from '../components/music/TrackCard';
import {TrackStatusBadges} from '../components/music/TrackStatusBadges';
import { UploadKindDot } from '../components/music/UploadKindDot';
import { HorizontalScroll } from '../components/ui/HorizontalScroll';
import { Skeleton } from '../components/ui/Skeleton';
import { preloadTrack } from '../lib/audio';
import { ago, art, dur, fc } from '../lib/formatters';
import type { FeedItem, Playlist, SCUser } from '../lib/hooks';
import {
  useDiscoverData,
  useFallbackTracks,
  useFeatured,
  useFollowingTracks,
  useLikedTracks,
  useRecommendedTracks,
  useRelatedPool,
} from '../lib/hooks';
import {
  ChevronRight,
  Compass,
  Headphones,
  Heart,
  ListMusic,
  Loader2,
  Music,
  pauseBlack18,
  pauseBlack22,
  playBlack18,
  playBlack22,
  Repeat2,
  Sparkles,
} from '../lib/icons';
import {usePerfMode} from '../lib/perf';
import {useScdMeta} from '../lib/scdMeta';
import { getArtistTarget, useArtistDisplay, useDisplayTitle } from '../lib/track-display';
import { useAutoHide } from '../lib/useAutoHide';
import { useTrackPlay } from '../lib/useTrackPlay';
import { useAuthStore } from '../stores/auth';
import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';

/* ── Helpers ──────────────────────────────────────────────── */

// Retained-card cap per horizontal shelf in reduced perf modes; full set lives
// behind "see all". Beauty keeps every card (byte-identical to today).
const SHELF_CAP = 24;

function useShelfCap(): number {
    return usePerfMode().mode === 'beauty' ? Number.POSITIVE_INFINITY : SHELF_CAP;
}

/* Blurred artwork backdrop for featured heroes; flat tint when blur is gated off. */
const HeroBlurBg = React.memo(function HeroBlurBg({cover}: { cover: string }) {
    const perf = usePerfMode();
    const b = perf.blur(80);
    return (
        <div className="absolute inset-0 pointer-events-none">
            {b > 0 && (
                <img
                    src={cover}
                    alt=""
                    className="w-full h-full object-cover scale-[1.4] opacity-20"
                    decoding="async"
                    style={{filter: `blur(${b}px) saturate(1.5)`}}
                />
            )}
            <div
                className="absolute inset-0 bg-gradient-to-r from-[rgb(8,8,10)]/70 via-[rgb(8,8,10)]/50 to-[rgb(8,8,10)]/70"/>
        </div>
    );
});

function greetingKey() {
  const h = new Date().getHours();
  if (h < 6) return 'home.goodNight';
  if (h < 12) return 'home.goodMorning';
  if (h < 18) return 'home.goodAfternoon';
  return 'home.goodEvening';
}

/* ── Section Header ───────────────────────────────────────── */

function SectionHeader({
  title,
  icon,
  onSeeAll,
}: {
  title: string;
  icon: React.ReactNode;
  onSeeAll?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between mb-5">
      <div className="flex items-center gap-2.5">
        <div className="w-8 h-8 rounded-xl bg-white/[0.04] border border-white/[0.06] flex items-center justify-center">
          {icon}
        </div>
        <h2 className="text-[15px] font-semibold tracking-tight text-white/90">{title}</h2>
      </div>
      {onSeeAll && (
        <button
          type="button"
          onClick={onSeeAll}
          className="flex items-center gap-1 text-[11px] text-white/30 hover:text-white/60 transition-colors duration-200 cursor-pointer"
        >
          {t('common.seeAll')}
          <ChevronRight size={12} />
        </button>
      )}
    </div>
  );
}

/* ── Skeletons ────────────────────────────────────────────── */

function ShelfSkeleton({ count = 8 }: { count?: number }) {
  return (
    <>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="w-[180px] shrink-0">
          <Skeleton className="aspect-square w-full" rounded="lg" />
          <Skeleton className="h-4 w-3/4 mt-2.5" rounded="sm" />
          <Skeleton className="h-3 w-1/2 mt-1.5" rounded="sm" />
        </div>
      ))}
    </>
  );
}

function FeaturedSkeleton() {
  return (
    <div className="glass rounded-3xl p-6 flex items-center gap-6">
      <Skeleton className="w-[160px] h-[160px] shrink-0" rounded="lg" />
      <div className="flex-1 space-y-3">
        <Skeleton className="h-6 w-3/4" rounded="sm" />
        <Skeleton className="h-4 w-1/3" rounded="sm" />
        <div className="pt-3" />
        <Skeleton className="h-3 w-1/2" rounded="sm" />
      </div>
      <Skeleton className="w-14 h-14 shrink-0" rounded="full" />
    </div>
  );
}

/* ── Featured Card (hero, first feed track) ───────────────── */

const FeaturedCard = React.memo(
  function FeaturedCard({ item, queue }: { item: FeedItem; queue: Track[] }) {
    const { t } = useTranslation();
    const track = item.origin as Track;
    const { isThisPlaying, togglePlay } = useTrackPlay(track, queue);
    const showPlayingOverlay = useAutoHide(isThisPlaying);
    const navigate = useNavigate();
    const isRepost = item.type.includes('repost');
    const cover = art(track.artwork_url);
    const avatar = art(track.user.avatar_url, 'small');

    return (
      <div
        className="relative rounded-3xl overflow-hidden group glass-featured select-none"
        onMouseEnter={() => preloadTrack(track.urn)}
      >
        {/* Blurred artwork background */}
          {cover && <HeroBlurBg cover={cover}/>}

        {/* Content */}
        <div className="relative flex items-center gap-6 p-6">
          {/* Artwork */}
          <div
            className="relative w-[160px] h-[160px] rounded-2xl overflow-hidden shrink-0 shadow-2xl ring-1 ring-white/[0.1] cursor-pointer group/cover"
            onClick={togglePlay}
          >
            {cover ? (
              <img
                src={cover}
                alt={track.title}
                className="w-full h-full object-cover transition-transform duration-500 ease-[var(--ease-apple)] group-hover/cover:scale-[1.05]"
                decoding="async"
                fetchPriority="high"
              />
            ) : (
              <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
                <Music size={40} className="text-white/15" />
              </div>
            )}

            {/* Hover play overlay on artwork */}
            <div
              className={`absolute inset-0 flex items-center justify-center transition-all duration-300 group-hover/cover:bg-black/30 group-hover/cover:opacity-100 ${
                showPlayingOverlay ? 'bg-black/30 opacity-100' : 'bg-black/0 opacity-0'
              }`}
            >
              <div
                className={`w-12 h-12 rounded-full flex items-center justify-center shadow-xl transition-all duration-300 ease-[var(--ease-apple)] group-hover/cover:scale-100 ${
                  showPlayingOverlay ? 'bg-white scale-100' : 'bg-white/90 scale-75'
                }`}
              >
                {isThisPlaying ? pauseBlack18 : playBlack18}
              </div>
            </div>

              <div className="absolute bottom-2 left-2 flex">
                  <TrackStatusBadges meta={track._scd_meta} variant="overlay"/>
              </div>
          </div>

          {/* Track info */}
          <div className="flex-1 min-w-0 py-1">
            {isRepost && (
              <div className="flex items-center gap-1.5 mb-2.5 text-[11px] text-white/30 font-medium">
                <Repeat2 size={11} />
                <span>{t('home.reposted')}</span>
                <span className="text-white/15">·</span>
                <span>{ago(item.created_at)}</span>
              </div>
            )}

            <FeaturedTitleArtist track={track} avatar={avatar} navigate={navigate} />

            <div className="flex items-center gap-3 mt-4 flex-wrap">
              {track.genre && (
                <span className="text-[10px] font-medium px-2.5 py-1 rounded-full bg-white/[0.06] text-white/45 border border-white/[0.06]">
                  {track.genre}
                </span>
              )}
              <div className="flex items-center gap-3 text-[11px] text-white/25 tabular-nums">
                <span className="flex items-center gap-1">
                  <Headphones size={11} />
                  {fc(track.playback_count)}
                </span>
                <span className="flex items-center gap-1">
                  <Heart size={11} />
                  {fc(track.favoritings_count ?? track.likes_count)}
                </span>
                <span>{dur(track.duration)}</span>
                {!isRepost && <span>{ago(item.created_at)}</span>}
              </div>
            </div>
          </div>

          {/* Large play button */}
          <button
            type="button"
            onClick={togglePlay}
            className={`w-14 h-14 rounded-full flex items-center justify-center shrink-0 transition-all duration-300 ease-[var(--ease-apple)] shadow-xl cursor-pointer ${
              isThisPlaying
                ? 'bg-white scale-100'
                : 'bg-white/90 hover:bg-white hover:scale-105 active:scale-95'
            }`}
          >
            {isThisPlaying ? pauseBlack22 : playBlack22}
          </button>
        </div>
      </div>
    );
  },
  (prev, next) => prev.item.origin.urn === next.item.origin.urn,
);

const FeaturedTitleArtist = React.memo(function FeaturedTitleArtist({
  track,
  avatar,
  navigate,
}: {
  track: Track;
  avatar: string | null;
  navigate: ReturnType<typeof useNavigate>;
}) {
  const artistDisplay = useArtistDisplay(track);
  const displayTitle = useDisplayTitle(track);
  const artistTarget = getArtistTarget(track);
  return (
    <>
      <h2
        className="text-xl font-bold text-white/95 truncate leading-tight cursor-pointer hover:text-white transition-colors duration-200"
        onClick={() => navigate(`/track/${encodeURIComponent(track.urn)}`)}
      >
        {displayTitle}
      </h2>
      <div
        className="flex items-center gap-2 mt-2 cursor-pointer group/artist"
        onClick={artistTarget ? () => navigate(artistTarget) : undefined}
      >
        {avatar && (
          <img
            src={avatar}
            alt=""
            className="w-5 h-5 rounded-full ring-1 ring-white/[0.08] group-hover/artist:ring-white/[0.15] transition-all duration-150"
            decoding="async"
          />
        )}
        <UploadKindDot kind={artistDisplay.uploadKind} />
        <p className="text-[13px] text-white/40 truncate group-hover/artist:text-white/60 transition-colors duration-150">
          {artistDisplay.primary}
        </p>
      </div>
    </>
  );
});

/* ── Feed Track Card (compact horizontal) ─────────────────── */

/* ── Feed Playlist Card ───────────────────────────────────── */

/* ── Featured Playlist Hero ───────────────────────────────── */

const FeaturedPlaylistHero = React.memo(function FeaturedPlaylistHero({
  playlist,
}: {
  playlist: Playlist;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const [loading, setLoading] = useState(false);

  const trackUrns = useMemo(
    () => new Set((playlist.tracks ?? []).map((t: Track) => t.urn)),
    [playlist.tracks],
  );
  const { isPausedFromThis, isPlayingFromThis } = usePlayerStore(
    useShallow((s) => ({
      isPlayingFromThis: s.isPlaying && s.currentTrack != null && trackUrns.has(s.currentTrack.urn),
      isPausedFromThis: !s.isPlaying && s.currentTrack != null && trackUrns.has(s.currentTrack.urn),
    })),
  );

  const cover = art(playlist.artwork_url) ?? art(playlist.tracks?.[0]?.artwork_url);

  const handlePlay = async () => {
    const { play, pause, resume } = usePlayerStore.getState();
    if (isPlayingFromThis) {
      pause();
      return;
    }
    if (isPausedFromThis) {
      resume();
      return;
    }

    if (playlist.tracks && playlist.tracks.length > 0) {
      play(playlist.tracks[0], playlist.tracks);
      return;
    }

    setLoading(true);
    try {
      const data = await import('../lib/api').then((m) =>
        m.api<{ collection: Track[] }>(`/playlists/${encodeURIComponent(playlist.urn)}/tracks`),
      );
      if (data.collection.length > 0) {
        usePlayerStore.getState().play(data.collection[0], data.collection);
      }
    } catch {
      navigate(`/playlist/${encodeURIComponent(playlist.urn)}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="relative rounded-3xl overflow-hidden group glass-featured select-none">
        {cover && <HeroBlurBg cover={cover}/>}

      <div className="relative flex items-center gap-6 p-6">
        <div
          className="relative w-[160px] h-[160px] rounded-2xl overflow-hidden shrink-0 shadow-2xl ring-1 ring-white/[0.1] cursor-pointer group/cover"
          onClick={handlePlay}
        >
          {cover ? (
            <img
              src={cover}
              alt={playlist.title}
              className="w-full h-full object-cover transition-transform duration-500 ease-[var(--ease-apple)] group-hover/cover:scale-[1.05]"
              decoding="async"
              fetchPriority="high"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
              <ListMusic size={40} className="text-white/15" />
            </div>
          )}
          <div
            className={`absolute inset-0 flex items-center justify-center transition-all duration-300 ${isPlayingFromThis ? 'bg-black/30 opacity-100' : 'bg-black/0 opacity-0 group-hover/cover:bg-black/30 group-hover/cover:opacity-100'}`}
          >
            {loading ? (
              <Loader2 size={24} className="text-white animate-spin" />
            ) : (
              <div
                className={`w-12 h-12 rounded-full flex items-center justify-center shadow-xl transition-all duration-300 ease-[var(--ease-apple)] ${isPlayingFromThis ? 'bg-white scale-100' : 'bg-white/90 scale-75 group-hover/cover:scale-100'}`}
              >
                {isPlayingFromThis ? pauseBlack18 : playBlack18}
              </div>
            )}
          </div>
          {playlist.track_count != null && (
            <div className="absolute bottom-2 right-2 flex items-center gap-0.5 text-[10px] font-medium bg-black/50 backdrop-blur-md text-white/70 px-2 py-0.5 rounded-full">
              <ListMusic size={10} />
              {playlist.track_count}
            </div>
          )}
        </div>

        <div className="flex-1 min-w-0 py-1">
          <div className="flex items-center gap-1.5 mb-2.5 text-[11px] text-white/30 font-medium">
            <ListMusic size={11} />
            <span>{t('search.playlists')}</span>
          </div>

          <h2
            className="text-xl font-bold text-white/95 truncate leading-tight cursor-pointer hover:text-white transition-colors duration-200"
            onClick={() => navigate(`/playlist/${encodeURIComponent(playlist.urn)}`)}
          >
            {playlist.title}
          </h2>

          {playlist.user && (
            <div
              className="flex items-center gap-2 mt-2 cursor-pointer group/artist"
              onClick={() => navigate(`/user/${encodeURIComponent(playlist.user.urn)}`)}
            >
              {playlist.user.avatar_url && (
                <img
                  src={art(playlist.user.avatar_url, 'small')!}
                  alt=""
                  className="w-5 h-5 rounded-full ring-1 ring-white/[0.08] group-hover/artist:ring-white/[0.15] transition-all duration-150"
                  decoding="async"
                />
              )}
              <p className="text-[13px] text-white/40 truncate group-hover/artist:text-white/60 transition-colors duration-150">
                {playlist.user.username}
              </p>
            </div>
          )}

          <div className="flex items-center gap-3 mt-4 text-[11px] text-white/25 tabular-nums">
            {playlist.genre && (
              <span className="text-[10px] font-medium px-2.5 py-1 rounded-full bg-white/[0.06] text-white/45 border border-white/[0.06]">
                {playlist.genre}
              </span>
            )}
            <span className="flex items-center gap-1">
              <ListMusic size={11} />
              {playlist.track_count ?? 0} {t('search.tracks').toLowerCase()}
            </span>
            <span className="flex items-center gap-1">
              <Heart size={11} />
              {fc(playlist.likes_count)}
            </span>
          </div>
        </div>

        <button
          type="button"
          onClick={handlePlay}
          className={`w-14 h-14 rounded-full flex items-center justify-center shrink-0 transition-all duration-300 ease-[var(--ease-apple)] shadow-xl cursor-pointer ${isPlayingFromThis ? 'bg-white scale-100' : 'bg-white/90 hover:bg-white hover:scale-105 active:scale-95'}`}
        >
          {loading ? (
            <Loader2 size={22} className="text-black animate-spin" />
          ) : isPlayingFromThis ? (
            pauseBlack22
          ) : (
            playBlack22
          )}
        </button>
      </div>
    </div>
  );
});

/* ── Featured User Hero ──────────────────────────────────── */

const FeaturedUserHero = React.memo(function FeaturedUserHero({ user }: { user: SCUser }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const avatar = art(user.avatar_url);

  return (
    <div
      className="relative rounded-3xl overflow-hidden group glass-featured select-none cursor-pointer"
      onClick={() => navigate(`/user/${encodeURIComponent(user.urn)}`)}
    >
        {avatar && <HeroBlurBg cover={avatar}/>}

      <div className="relative flex items-center gap-6 p-6">
        <div className="w-[160px] h-[160px] rounded-full overflow-hidden shrink-0 shadow-2xl ring-1 ring-white/[0.1]">
          {avatar ? (
            <img
              src={avatar}
              alt={user.username}
              className="w-full h-full object-cover"
              decoding="async"
              fetchPriority="high"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
              <Music size={40} className="text-white/15" />
            </div>
          )}
        </div>

        <div className="flex-1 min-w-0 py-1">
          <h2 className="text-xl font-bold text-white/95 truncate leading-tight group-hover:text-white transition-colors duration-200">
            {user.username}
          </h2>

          {(user.city || user.country) && (
            <p className="text-[13px] text-white/30 mt-1.5">
              {[user.city, user.country].filter(Boolean).join(', ')}
            </p>
          )}

          <div className="flex items-center gap-4 mt-4 text-[11px] text-white/25 tabular-nums">
            {user.followers_count != null && (
              <span>
                {fc(user.followers_count)} {t('user.followers')}
              </span>
            )}
            {user.track_count != null && (
              <span>
                {fc(user.track_count)} {t('search.tracks').toLowerCase()}
              </span>
            )}
          </div>
        </div>

        <ChevronRight
          size={28}
          className="text-white/20 shrink-0 group-hover:text-white/40 transition-colors"
        />
      </div>
    </div>
  );
});

/* ── Isolated Sections ────────────────────────────────────── */

const FeaturedHero = React.memo(function FeaturedHero() {
  const { data: featured, isLoading: featuredLoading } = useFeatured();

    if (featuredLoading) return <FeaturedSkeleton/>;
    if (!featured) return null;

  // Admin-pinned content
    switch (featured.type) {
        case 'track':
            return (
                <section>
                    <FeaturedCard
                        item={{type: 'track', created_at: '', origin: featured.data as Track}}
                        queue={[featured.data as Track]}
                    />
                </section>
            );
        case 'playlist':
            return (
                <section>
                    <FeaturedPlaylistHero playlist={featured.data as Playlist}/>
                </section>
            );
        case 'user':
            return (
                <section>
                    <FeaturedUserHero user={featured.data as SCUser}/>
                </section>
            );
        default:
            return null;
    }
});

const FallbackShelf = React.memo(function FallbackShelf() {
  const { t } = useTranslation();
    const shelfCap = useShelfCap();
  const user = useAuthStore((s) => s.user);

  // If user has any likes or followings, they're not a new user — no fallback needed
  const hasActivity = (user?.public_favorites_count ?? 0) > 0 || (user?.followings_count ?? 0) > 0;

  const { data: fallbackData, isLoading: fallbackLoading } = useFallbackTracks();
  const fallbackTracks = useMemo(() => fallbackData?.collection ?? [], [fallbackData]);

  if (hasActivity || (!fallbackLoading && fallbackTracks.length === 0)) return null;

  return (
    <>
      {/* Hint to start liking */}
      <section className="glass-flat rounded-2xl p-5 flex items-center gap-4">
        <div className="w-10 h-10 rounded-xl bg-accent/10 border border-accent/20 flex items-center justify-center shrink-0">
          <Heart size={18} className="text-accent" />
        </div>
        <div>
          <p className="text-[13px] font-medium text-white/80">{t('home.startLikingTitle')}</p>
          <p className="text-[11px] text-white/35 mt-0.5">{t('home.startLikingDesc')}</p>
        </div>
      </section>

      <section>
        <SectionHeader
          title={t('home.startListening', 'Start Listening')}
          icon={<Headphones size={15} className="text-accent" />}
        />
        <HorizontalScroll>
          {fallbackLoading ? (
            <ShelfSkeleton count={6} />
          ) : (
              fallbackTracks.slice(0, shelfCap).map((track) => (
              <div key={track.urn} className="w-[180px] shrink-0">
                <TrackCard track={track} queue={fallbackTracks} />
              </div>
            ))
          )}
        </HorizontalScroll>
      </section>
    </>
  );
});

const LikedShelf = React.memo(function LikedShelf({
  likedTracks,
  isLoading,
}: {
  likedTracks: Track[];
  isLoading: boolean;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
    const shelfCap = useShelfCap();

  if (!isLoading && likedTracks.length === 0) return null;

  return (
    <section>
      <SectionHeader
        title={t('library.likedTracks')}
        icon={<Heart size={15} className="text-accent" />}
        onSeeAll={() => navigate('/library')}
      />
      <HorizontalScroll>
        {isLoading ? (
          <ShelfSkeleton />
        ) : (
            likedTracks.slice(0, shelfCap).map((track) => (
            <div key={track.urn} className="w-[180px] shrink-0">
              <TrackCard track={track} queue={likedTracks} />
            </div>
          ))
        )}
      </HorizontalScroll>
    </section>
  );
});

const FollowingShelf = React.memo(function FollowingShelf({
  followingTracks,
  isLoading,
}: {
  followingTracks: Track[];
  isLoading: boolean;
}) {
  const { t } = useTranslation();
    const shelfCap = useShelfCap();

  if (!isLoading && followingTracks.length === 0) return null;

  return (
    <section>
      <SectionHeader
        title={t('home.freshReleases')}
        icon={<Music size={15} className="text-white/50" />}
      />
      <HorizontalScroll>
        {isLoading ? (
          <ShelfSkeleton />
        ) : (
            followingTracks.slice(0, shelfCap).map((track) => (
            <div key={track.urn} className="w-[180px] shrink-0">
              <TrackCard track={track} queue={followingTracks} />
            </div>
          ))
        )}
      </HorizontalScroll>
    </section>
  );
});

const DiscoverSection = React.memo(function DiscoverSection({
  likedTracks,
}: {
  likedTracks: Track[];
}) {
  const { t } = useTranslation();
    const shelfCap = useShelfCap();
  const { data: pool, isLoading } = useRelatedPool(likedTracks);

  // ── Recommended ──
    const recommendedRaw = useRecommendedTracks(pool, 40);
    const recommendedTracks = useScdMeta(recommendedRaw);

  // ── Discover by genre ──
  const discoverData = useDiscoverData(pool, likedTracks);
  const [activeGenre, setActiveGenre] = useState<string | null>(null);
  const genres = useMemo(() => discoverData.map((d) => d.genre), [discoverData]);
  const selectedGenre =
    activeGenre && genres.includes(activeGenre) ? activeGenre : (genres[0] ?? null);
    const genreRaw = useMemo(
    () => discoverData.find((d) => d.genre === selectedGenre)?.tracks ?? [],
    [discoverData, selectedGenre],
  );
    const genreTracks = useScdMeta(genreRaw);

  return (
    <>
      {/* Recommended For You */}
      {(isLoading || recommendedTracks.length > 0) && (
        <section>
          <SectionHeader
            title={t('home.recommended', 'Recommended For You')}
            icon={<Sparkles size={15} className="text-amber-400/70" />}
          />
          <HorizontalScroll>
            {isLoading ? (
              <ShelfSkeleton />
            ) : (
                recommendedTracks.slice(0, shelfCap).map((track) => (
                <div key={track.urn} className="w-[180px] shrink-0">
                  <TrackCard track={track} queue={recommendedTracks} />
                </div>
              ))
            )}
          </HorizontalScroll>
        </section>
      )}

      {/* Discover by genre */}
      {(isLoading || genres.length > 0) && (
        <section>
          <SectionHeader
            title={t('home.discover', 'Discover')}
            icon={<Compass size={15} className="text-cyan-400/70" />}
          />
          <div className="flex items-center gap-1.5 mb-4 flex-wrap">
            {genres.map((g) => (
              <button
                key={g}
                type="button"
                onClick={() => setActiveGenre(g)}
                className={`px-3.5 py-1.5 rounded-full text-[12px] font-medium transition-all duration-200 cursor-pointer capitalize ${
                  selectedGenre === g
                    ? 'bg-white/[0.12] text-white border border-white/[0.08]'
                    : 'bg-white/[0.03] text-white/40 border border-white/[0.04] hover:bg-white/[0.06] hover:text-white/60'
                }`}
              >
                {g}
              </button>
            ))}
          </div>
          <HorizontalScroll>
            {isLoading ? (
              <ShelfSkeleton />
            ) : (
                genreTracks.slice(0, shelfCap).map((track) => (
                <div key={track.urn} className="w-[180px] shrink-0">
                  <TrackCard track={track} queue={genreTracks} />
                </div>
              ))
            )}
          </HorizontalScroll>
        </section>
      )}
    </>
  );
});

/* ── Home Page ────────────────────────────────────────────── */

export function Home() {
  const { t } = useTranslation();
  const user = useAuthStore((s) => s.user);
  const likedTracksQuery = useLikedTracks(100);
  const followingQuery = useFollowingTracks(20);

  const followingTracks = useMemo(
    () => followingQuery.data?.collection ?? [],
    [followingQuery.data],
  );
  const likedShelfTracks = useMemo(
    () => likedTracksQuery.tracks.slice(0, 50),
    [likedTracksQuery.tracks],
  );

  return (
    <div className="p-6 pb-4 space-y-8">
      {/* Hero Greeting — no data hooks, won't re-render */}
      <section className="pt-1">
        <h1 className="hero-greeting text-3xl font-bold tracking-tight leading-tight pb-1">
          {t(greetingKey())}
          {user?.username ? `, ${user.username}` : ''}
        </h1>
        <div className="mt-3 h-px bg-gradient-to-r from-white/[0.06] via-white/[0.03] to-transparent" />
      </section>

      {/* SoundWave — AI-powered recommendations, at the very top */}
      <div className="relative">
        <SoundWaveBlock />
        <SoundWaveLockOverlay />
      </div>

      {/* Each section is isolated — own hooks, own re-render boundary */}
        <FeaturedHero/>
      <FallbackShelf />
      <LikedShelf likedTracks={likedShelfTracks} isLoading={likedTracksQuery.isLoading} />
      <FollowingShelf followingTracks={followingTracks} isLoading={followingQuery.isLoading} />
      <DiscoverSection likedTracks={likedTracksQuery.tracks} />
    </div>
  );
}
