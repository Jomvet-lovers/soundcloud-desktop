import { useState } from "react";
import { useTranslation } from "react-i18next";
import {
  Heart,
  ChevronRight,
  Loader2,
  Music,
  Repeat2,
  ListMusic,
  Play,
  Pause,
  Headphones,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import {
  useFeed,
  useLikedTracks,
  useFollowingTracks,
  useInfiniteScroll,
} from "../lib/hooks";
import type { FeedItem } from "../lib/hooks";
import { TrackCard } from "../components/music/TrackCard";
import { HorizontalScroll } from "../components/ui/HorizontalScroll";
import { Skeleton } from "../components/ui/Skeleton";
import { useAuthStore } from "../stores/auth";
import { usePlayerStore } from "../stores/player";
import type { Track } from "../stores/player";
import { preloadTrack } from "../lib/audio";

/* ── Helpers ──────────────────────────────────────────────── */

function greetingKey() {
  const h = new Date().getHours();
  if (h < 6) return "home.goodNight";
  if (h < 12) return "home.goodMorning";
  if (h < 18) return "home.goodAfternoon";
  return "home.goodEvening";
}

function fc(n?: number) {
  if (!n) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

function dur(ms: number) {
  const s = Math.floor(ms / 1000);
  return `${Math.floor(s / 60)}:${(s % 60).toString().padStart(2, "0")}`;
}

function art(url: string | null | undefined, size = "t500x500") {
  return url?.replace("-large", `-${size}`) ?? null;
}

function ago(dateStr: string) {
  const d = new Date(dateStr.replace(/\//g, "-").replace(" +0000", "Z"));
  const s = Math.floor((Date.now() - d.getTime()) / 1000);
  if (s < 60) return "now";
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h`;
  const dd = Math.floor(h / 24);
  if (dd < 7) return `${dd}d`;
  const w = Math.floor(dd / 7);
  if (w < 5) return `${w}w`;
  return `${Math.floor(dd / 30)}mo`;
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
        <h2 className="text-[15px] font-semibold tracking-tight text-white/90">
          {title}
        </h2>
      </div>
      {onSeeAll && (
        <button
          type="button"
          onClick={onSeeAll}
          className="flex items-center gap-1 text-[11px] text-white/30 hover:text-white/60 transition-colors duration-200 cursor-pointer"
        >
          {t("common.seeAll")}
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

function FeedSkeleton({ count = 6 }: { count?: number }) {
  return (
    <div className="grid grid-cols-[repeat(auto-fill,minmax(380px,1fr))] gap-2.5">
      {Array.from({ length: count }).map((_, i) => (
        <div
          key={i}
          className="glass rounded-2xl p-3 flex items-center gap-3.5"
        >
          <Skeleton className="w-[76px] h-[76px] shrink-0" rounded="lg" />
          <div className="flex-1 space-y-2">
            <Skeleton className="h-4 w-3/4" rounded="sm" />
            <Skeleton className="h-3 w-1/2" rounded="sm" />
            <Skeleton className="h-2.5 w-2/5" rounded="sm" />
          </div>
        </div>
      ))}
    </div>
  );
}

/* ── Featured Card (hero, first feed track) ───────────────── */

function FeaturedCard({
  item,
  queue,
}: {
  item: FeedItem;
  queue: Track[];
}) {
  const { t } = useTranslation();
  const { play, pause, resume, currentTrack, isPlaying } = usePlayerStore();
  const navigate = useNavigate();
  const track = item.origin as Track;
  const isThis = currentTrack?.urn === track.urn;
  const isRepost = item.type.includes("repost");
  const cover = art(track.artwork_url);
  const avatar = art(track.user.avatar_url, "small");

  const handlePlay = () => {
    if (isThis && isPlaying) pause();
    else if (isThis) resume();
    else play(track, queue);
  };

  return (
    <div
      className="relative rounded-3xl overflow-hidden group glass-featured animate-fade-in-up"
      onMouseEnter={() => preloadTrack(track.urn)}
    >
      {/* Blurred artwork background */}
      {cover && (
        <div className="absolute inset-0 pointer-events-none">
          <img
            src={cover}
            alt=""
            className="w-full h-full object-cover scale-[1.4] blur-[80px] opacity-20 saturate-150"
          />
          <div className="absolute inset-0 bg-gradient-to-r from-[rgb(8,8,10)]/70 via-[rgb(8,8,10)]/50 to-[rgb(8,8,10)]/70" />
        </div>
      )}

      {/* Content */}
      <div className="relative flex items-center gap-6 p-6">
        {/* Artwork */}
        <div
          className="relative w-[160px] h-[160px] rounded-2xl overflow-hidden shrink-0 shadow-2xl ring-1 ring-white/[0.1] cursor-pointer group/cover"
          onClick={handlePlay}
        >
          {cover ? (
            <img
              src={cover}
              alt={track.title}
              className="w-full h-full object-cover transition-transform duration-500 ease-[var(--ease-apple)] group-hover/cover:scale-[1.05]"
            />
          ) : (
            <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
              <Music size={40} className="text-white/15" />
            </div>
          )}

          {/* Hover play overlay on artwork */}
          <div
            className={`absolute inset-0 flex items-center justify-center transition-all duration-300 ${
              isThis && isPlaying
                ? "bg-black/30 opacity-100"
                : "bg-black/0 opacity-0 group-hover/cover:bg-black/30 group-hover/cover:opacity-100"
            }`}
          >
            <div
              className={`w-12 h-12 rounded-full flex items-center justify-center shadow-xl transition-all duration-300 ease-[var(--ease-apple)] ${
                isThis && isPlaying
                  ? "bg-white scale-100"
                  : "bg-white/90 scale-75 group-hover/cover:scale-100"
              }`}
            >
              {isThis && isPlaying ? (
                <Pause size={18} fill="black" strokeWidth={0} />
              ) : (
                <Play
                  size={18}
                  fill="black"
                  strokeWidth={0}
                  className="ml-0.5"
                />
              )}
            </div>
          </div>
        </div>

        {/* Track info */}
        <div className="flex-1 min-w-0 py-1">
          {isRepost && (
            <div className="flex items-center gap-1.5 mb-2.5 text-[11px] text-white/30 font-medium">
              <Repeat2 size={11} />
              <span>{t("home.reposted")}</span>
              <span className="text-white/15">·</span>
              <span>{ago(item.created_at)}</span>
            </div>
          )}

          <h2
            className="text-xl font-bold text-white/95 truncate leading-tight cursor-pointer hover:text-white transition-colors duration-200"
            onClick={() =>
              navigate(`/track/${encodeURIComponent(track.urn)}`)
            }
          >
            {track.title}
          </h2>

          <div
            className="flex items-center gap-2 mt-2 cursor-pointer group/artist"
            onClick={() => navigate(`/user/${encodeURIComponent(track.user.urn)}`)}
          >
            {avatar && (
              <img
                src={avatar}
                alt=""
                className="w-5 h-5 rounded-full ring-1 ring-white/[0.08] group-hover/artist:ring-white/[0.15] transition-all duration-150"
              />
            )}
            <p className="text-[13px] text-white/40 truncate group-hover/artist:text-white/60 transition-colors duration-150">
              {track.user.username}
            </p>
          </div>

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
          onClick={handlePlay}
          className={`w-14 h-14 rounded-full flex items-center justify-center shrink-0 transition-all duration-300 ease-[var(--ease-apple)] shadow-xl cursor-pointer ${
            isThis && isPlaying
              ? "bg-white scale-100"
              : "bg-white/90 hover:bg-white hover:scale-105 active:scale-95"
          }`}
        >
          {isThis && isPlaying ? (
            <Pause size={22} fill="black" strokeWidth={0} />
          ) : (
            <Play
              size={22}
              fill="black"
              strokeWidth={0}
              className="ml-0.5"
            />
          )}
        </button>
      </div>
    </div>
  );
}

/* ── Feed Track Card (compact horizontal) ─────────────────── */

function FeedTrackCard({
  item,
  queue,
}: {
  item: FeedItem;
  queue: Track[];
}) {
  const { t } = useTranslation();
  const { play, pause, resume, currentTrack, isPlaying } = usePlayerStore();
  const navigate = useNavigate();
  const track = item.origin as Track;
  const isThis = currentTrack?.urn === track.urn;
  const isRepost = item.type.includes("repost");
  const cover = art(track.artwork_url, "t300x300");

  const handlePlay = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isThis && isPlaying) pause();
    else if (isThis) resume();
    else play(track, queue);
  };

  return (
    <div
      className={`group glass rounded-2xl p-3 flex items-center gap-3.5 transition-all duration-300 ease-[var(--ease-apple)] ${
        isThis
          ? "ring-1 ring-accent/20 bg-accent/[0.02]"
          : "hover:bg-white/[0.035]"
      }`}
      onMouseEnter={() => preloadTrack(track.urn)}
    >
      {/* Artwork */}
      <div
        className="relative w-[76px] h-[76px] rounded-xl overflow-hidden shrink-0 ring-1 ring-white/[0.06] cursor-pointer"
        onClick={handlePlay}
      >
        {cover ? (
          <img
            src={cover}
            alt={track.title}
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
            <Music size={22} className="text-white/15" />
          </div>
        )}

        {/* Play overlay */}
        <div
          className={`absolute inset-0 flex items-center justify-center transition-all duration-200 ${
            isThis && isPlaying
              ? "bg-black/30 opacity-100"
              : "bg-black/0 opacity-0 group-hover:bg-black/30 group-hover:opacity-100"
          }`}
        >
          <div
            className={`w-9 h-9 rounded-full flex items-center justify-center shadow-lg transition-all duration-200 ease-[var(--ease-apple)] ${
              isThis && isPlaying
                ? "bg-white scale-100"
                : "bg-white/90 scale-75 group-hover:scale-100"
            }`}
          >
            {isThis && isPlaying ? (
              <Pause size={14} fill="black" strokeWidth={0} />
            ) : (
              <Play
                size={14}
                fill="black"
                strokeWidth={0}
                className="ml-px"
              />
            )}
          </div>
        </div>
      </div>

      {/* Track info */}
      <div className="flex-1 min-w-0">
        {isRepost && (
          <div className="flex items-center gap-1 mb-1 text-[10px] text-white/20 font-medium">
            <Repeat2 size={9} />
            <span>{t("home.reposted")}</span>
          </div>
        )}
        <p
          className="text-[13px] font-medium text-white/90 truncate leading-snug cursor-pointer hover:text-white transition-colors duration-150"
          onClick={() =>
            navigate(`/track/${encodeURIComponent(track.urn)}`)
          }
        >
          {track.title}
        </p>
        <p
          className="text-[11px] text-white/35 truncate mt-0.5 cursor-pointer hover:text-white/55 transition-colors duration-150"
          onClick={() => navigate(`/user/${encodeURIComponent(track.user.urn)}`)}
        >
          {track.user.username}
        </p>
        <div className="flex items-center gap-2 mt-1.5 text-[10px] text-white/20 tabular-nums">
          {track.genre && (
            <span className="px-1.5 py-px rounded-full bg-white/[0.04] text-white/30 border border-white/[0.04] text-[9px]">
              {track.genre}
            </span>
          )}
          <span className="flex items-center gap-0.5">
            <Headphones size={9} />
            {fc(track.playback_count)}
          </span>
          <span className="flex items-center gap-0.5">
            <Heart size={9} />
            {fc(track.favoritings_count ?? track.likes_count)}
          </span>
        </div>
      </div>

      {/* Duration + time */}
      <div className="text-right shrink-0 self-center">
        <p className="text-[11px] text-white/30 tabular-nums font-medium">
          {dur(track.duration)}
        </p>
        <p className="text-[10px] text-white/15 mt-0.5">
          {ago(item.created_at)}
        </p>
      </div>
    </div>
  );
}

/* ── Feed Playlist Card ───────────────────────────────────── */

function FeedPlaylistCard({ item }: { item: FeedItem }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { play, pause, resume, currentTrack, isPlaying } = usePlayerStore();
  const [loading, setLoading] = useState(false);
  const origin = item.origin;
  const isRepost = item.type.includes("repost");
  const cover = art(origin.artwork_url, "t300x300");

  // Check if any track from this playlist is currently playing
  const isPlayingFromThis = currentTrack
    ? origin.tracks?.some?.((t: Track) => t.urn === currentTrack.urn)
    : false;

  const handlePlay = async (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isPlayingFromThis && isPlaying) {
      pause();
      return;
    }
    if (isPlayingFromThis) {
      resume();
      return;
    }

    // If inline tracks are available, use them directly
    if (origin.tracks && origin.tracks.length > 0) {
      play(origin.tracks[0], origin.tracks);
      return;
    }

    // Fetch tracks from API
    setLoading(true);
    try {
      const data = await import("../lib/api").then((m) =>
        m.api<{ collection: Track[] }>(
          `/playlists/${encodeURIComponent(origin.urn)}/tracks`,
        ),
      );
      const tracks = data.collection;
      if (tracks.length > 0) {
        play(tracks[0], tracks);
      }
    } catch {
      // fallback: navigate to playlist page
      navigate(`/playlist/${encodeURIComponent(origin.urn)}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div
      className={`group glass rounded-2xl p-3 flex items-center gap-3.5 transition-all duration-300 ease-[var(--ease-apple)] ${
        isPlayingFromThis
          ? "ring-1 ring-accent/20 bg-accent/[0.02]"
          : "hover:bg-white/[0.035]"
      }`}
    >
      {/* Artwork */}
      <div
        className="relative w-[76px] h-[76px] rounded-xl overflow-hidden shrink-0 ring-1 ring-white/[0.06] cursor-pointer"
        onClick={handlePlay}
      >
        {cover ? (
          <img
            src={cover}
            alt={origin.title}
            className="w-full h-full object-cover"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-white/[0.01]">
            <ListMusic size={22} className="text-white/15" />
          </div>
        )}

        {/* Play overlay */}
        <div
          className={`absolute inset-0 flex items-center justify-center transition-all duration-200 ${
            isPlayingFromThis && isPlaying
              ? "bg-black/30 opacity-100"
              : "bg-black/0 opacity-0 group-hover:bg-black/30 group-hover:opacity-100"
          }`}
        >
          {loading ? (
            <Loader2 size={16} className="text-white animate-spin" />
          ) : (
            <div
              className={`w-9 h-9 rounded-full flex items-center justify-center shadow-lg transition-all duration-200 ease-[var(--ease-apple)] ${
                isPlayingFromThis && isPlaying
                  ? "bg-white scale-100"
                  : "bg-white/90 scale-75 group-hover:scale-100"
              }`}
            >
              {isPlayingFromThis && isPlaying ? (
                <Pause size={14} fill="black" strokeWidth={0} />
              ) : (
                <Play
                  size={14}
                  fill="black"
                  strokeWidth={0}
                  className="ml-px"
                />
              )}
            </div>
          )}
        </div>

        {/* Track count pill */}
        {origin.track_count != null && (
          <div className="absolute bottom-1.5 right-1.5 flex items-center gap-0.5 text-[9px] font-medium bg-black/50 backdrop-blur-md text-white/70 px-1.5 py-0.5 rounded-full">
            <ListMusic size={8} />
            {origin.track_count}
          </div>
        )}
      </div>

      {/* Playlist info */}
      <div className="flex-1 min-w-0">
        {isRepost && (
          <div className="flex items-center gap-1 mb-1 text-[10px] text-white/20 font-medium">
            <Repeat2 size={9} />
            <span>{t("home.reposted")}</span>
          </div>
        )}
        <p
          className="text-[13px] font-medium text-white/90 truncate leading-snug cursor-pointer hover:text-white transition-colors duration-150"
          onClick={() =>
            navigate(`/playlist/${encodeURIComponent(origin.urn)}`)
          }
        >
          {origin.title}
        </p>
        <p
          className="text-[11px] text-white/35 truncate mt-0.5 cursor-pointer hover:text-white/55 transition-colors duration-150"
          onClick={() => origin.user?.urn && navigate(`/user/${encodeURIComponent(origin.user.urn)}`)}
        >
          {origin.user?.username}
        </p>
        <div className="flex items-center gap-2 mt-1.5 text-[10px] text-white/20">
          <span className="flex items-center gap-0.5">
            <ListMusic size={9} />
            {origin.track_count ?? 0} {t("search.tracks").toLowerCase()}
          </span>
        </div>
      </div>

      {/* Time */}
      <div className="text-right shrink-0 self-center">
        <p className="text-[10px] text-white/15">{ago(item.created_at)}</p>
      </div>
    </div>
  );
}

/* ── Home Page ────────────────────────────────────────────── */

export function Home() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const user = useAuthStore((s) => s.user);

  const {
    items: feedItems,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading: feedLoading,
  } = useFeed();
  const { data: likes, isLoading: likesLoading } = useLikedTracks(20);
  const { data: following, isLoading: followingLoading } =
    useFollowingTracks(20);

  const sentinelRef = useInfiniteScroll(
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  );

  const likedTracks = likes?.collection ?? [];
  const followingTracks = following?.collection ?? [];

  // First track in feed → featured hero card
  const featuredItem = feedItems.find((i) => i.type.includes("track"));
  const streamItems = feedItems.filter((i) => i !== featuredItem);

  // All feed tracks as queue context
  const feedTrackQueue = feedItems
    .filter((i) => i.type.includes("track"))
    .map((i) => i.origin as Track);

  return (
    <div className="p-6 pb-4 space-y-8">
      {/* ── Hero Greeting ──────────────────────────────── */}
      <section className="pt-1">
        <h1 className="text-3xl font-bold tracking-tight bg-gradient-to-r from-white via-white/80 to-accent/80 bg-clip-text text-transparent leading-tight pb-1">
          {t(greetingKey())}
          {user?.username ? `, ${user.username}` : ""}
        </h1>
        <div className="mt-3 h-px bg-gradient-to-r from-white/[0.06] via-white/[0.03] to-transparent" />
      </section>

      {/* ── Featured Track ─────────────────────────────── */}
      {feedLoading ? (
        <FeaturedSkeleton />
      ) : (
        featuredItem && (
          <section>
            <FeaturedCard item={featuredItem} queue={feedTrackQueue} />
          </section>
        )
      )}

      {/* ── Liked Tracks ───────────────────────────────── */}
      {(likesLoading || likedTracks.length > 0) && (
        <section>
          <SectionHeader
            title={t("library.likedTracks")}
            icon={<Heart size={15} className="text-accent" />}
            onSeeAll={() => navigate("/library")}
          />
          <HorizontalScroll>
            {likesLoading ? (
              <ShelfSkeleton />
            ) : (
              likedTracks.map((track) => (
                <div key={track.urn} className="w-[180px] shrink-0">
                  <TrackCard track={track} queue={likedTracks} />
                </div>
              ))
            )}
          </HorizontalScroll>
        </section>
      )}

      {/* ── Fresh Releases ─────────────────────────────── */}
      {(followingLoading || followingTracks.length > 0) && (
        <section>
          <SectionHeader
            title={t("home.freshReleases")}
            icon={<Music size={15} className="text-white/50" />}
          />
          <HorizontalScroll>
            {followingLoading ? (
              <ShelfSkeleton />
            ) : (
              followingTracks.map((track) => (
                <div key={track.urn} className="w-[180px] shrink-0">
                  <TrackCard track={track} queue={followingTracks} />
                </div>
              ))
            )}
          </HorizontalScroll>
        </section>
      )}

      {/* ── Feed Stream ────────────────────────────────── */}
      <section>
        <SectionHeader
          title={t("home.yourFeed")}
          icon={<Music size={15} className="text-white/50" />}
        />

        {feedLoading ? (
          <FeedSkeleton />
        ) : (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(380px,1fr))] gap-2.5">
            {streamItems.map((item, i) => (
              <div
                key={item.origin.urn}
                className="animate-fade-in-up"
                style={{ animationDelay: `${Math.min(i * 40, 400)}ms` }}
              >
                {item.type.includes("track") ? (
                  <FeedTrackCard
                    item={item}
                    queue={feedTrackQueue}
                  />
                ) : (
                  <FeedPlaylistCard
                    item={item}
                  />
                )}
              </div>
            ))}
          </div>
        )}

        {/* Sentinel for infinite scroll */}
        <div
          ref={sentinelRef}
          className="h-12 flex items-center justify-center"
        >
          {isFetchingNextPage && (
            <Loader2
              size={18}
              className="text-white/15 animate-spin"
            />
          )}
          {!feedLoading &&
            !hasNextPage &&
            !isFetchingNextPage &&
            streamItems.length > 0 && (
              <div className="flex items-center gap-2 text-[11px] text-white/15">
                <div className="h-px w-8 bg-white/[0.06]" />
                <span>{t("home.endOfFeed")}</span>
                <div className="h-px w-8 bg-white/[0.06]" />
              </div>
            )}
        </div>
      </section>
    </div>
  );
}
