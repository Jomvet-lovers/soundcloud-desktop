import { useState, useEffect } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Loader2,
  Users,
  MapPin,
  Calendar,
  Globe,
  Instagram,
  Twitter,
  Youtube,
  AlertCircle,
  Play,
  Pause,
  ListMusic,
  Heart,
  Headphones,
  Link as LinkIcon,
  Music
} from "lucide-react";
import { api } from "../lib/api";
import {
  useUser,
  useUserTracks,
  useUserPlaylists,
  useUserLikedTracks,
  useUserWebProfiles,
  useInfiniteScroll,
  type Playlist
} from "../lib/hooks";
import { useAuthStore } from "../stores/auth";
import { usePlayerStore, type Track } from "../stores/player";
import { preloadTrack } from "../lib/audio";

/* ── Helpers ──────────────────────────────────────────────── */

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

function dateFormatted(dateStr: string) {
  if (!dateStr) return "";
  const d = new Date(dateStr.replace(/\//g, "-").replace(" +0000", "Z"));
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
  });
}

function getWebIcon(service: string) {
  switch (service.toLowerCase()) {
    case "instagram": return <Instagram size={14} />;
    case "twitter": return <Twitter size={14} />;
    case "youtube": return <Youtube size={14} />;
    case "personal": return <Globe size={14} />;
    default: return <LinkIcon size={14} />;
  }
}

/* ── Follow Button ────────────────────────────────────────── */

function FollowBtn({ userUrn }: { userUrn: string }) {
  const { t } = useTranslation();

  const currentUser = useAuthStore((s) => s.user);
  const qc = useQueryClient();

  const { data: initialFollowing = false, isLoading: isQueryLoading } = useQuery({
    queryKey:["following", currentUser?.urn, userUrn],
    queryFn: () =>
      api<boolean>(
        `/users/${encodeURIComponent(currentUser!.urn)}/followings/${encodeURIComponent(userUrn)}`
      ),
    enabled: !!currentUser?.urn && !!userUrn,
  });

  const [following, setFollowing] = useState(false);
  const [loading, setLoading] = useState(false);
  
  useEffect(() => {
    setFollowing(initialFollowing);
  }, [initialFollowing]);

  const toggle = async () => {
    setLoading(true);
    const next = !following;
    setFollowing(next);
    try {
      await api(`/me/followings/${encodeURIComponent(userUrn)}`, {
        method: next ? "PUT" : "DELETE",
      });
      // update follwrs counts
      qc.invalidateQueries({ queryKey: ["following", currentUser?.urn, userUrn] });
      qc.invalidateQueries({ queryKey: ["user", userUrn] });
    } catch (e) {
      // Revert on failure
      setFollowing(!next);
    } finally {
      setLoading(false);
    }
  };

  return (
      <button
          onClick={toggle}
          disabled={loading || isQueryLoading}
          className={`cursor-pointer inline-flex items-center justify-center gap-2 px-6 py-2.5 rounded-xl text-sm font-semibold transition-all duration-300 ease-[var(--ease-apple)] shadow-xl disabled:opacity-50 ${
              following
                  ? "bg-white/[0.08] text-white hover:bg-white/[0.12] border border-white/[0.08]"
                  : "bg-white text-black hover:bg-white/90 hover:scale-105 active:scale-95 shadow-[0_0_20px_rgba(255,255,255,0.2)]"
          }`}
      >
        {loading || isQueryLoading ? (
            <Loader2 size={16} className="animate-spin" />
        ) : following ? (
            t("user.following")
        ) : (
            t("user.follow")
        )}
      </button>
  );
}

/* ── Track Row (For Tracks & Likes) ───────────────────────── */

function TrackRow({ track, index, queue }: { track: Track; index: number; queue: Track[] }) {
  const { play, pause, resume, currentTrack, isPlaying } = usePlayerStore();
  const navigate = useNavigate();
  const isThis = currentTrack?.urn === track.urn;
  const cover = art(track.artwork_url, "t200x200");

  const handlePlay = () => {
    if (isThis && isPlaying) pause();
    else if (isThis) resume();
    else play(track, queue);
  };

  return (
      <div
          className={`group flex items-center gap-4 px-4 py-3 rounded-2xl transition-all duration-300 ease-[var(--ease-apple)] ${
              isThis
                  ? "bg-accent/[0.06] ring-1 ring-accent/20 shadow-[inset_0_0_20px_rgba(255,85,0,0.05)]"
                  : "hover:bg-white/[0.04]"
          }`}
          onMouseEnter={() => preloadTrack(track.urn)}
      >
        {/* Index & Play */}
        <div
            className="w-8 h-8 flex items-center justify-center shrink-0 cursor-pointer"
            onClick={handlePlay}
        >
          {isThis && isPlaying ? (
              <div className="w-8 h-8 rounded-full bg-accent flex items-center justify-center shadow-[0_0_15px_var(--color-accent-glow)] scale-100 animate-fade-in-up">
                <Pause size={14} fill="white" strokeWidth={0} />
              </div>
          ) : (
              <>
            <span className="text-[13px] text-white/20 tabular-nums font-medium group-hover:hidden">
              {index + 1}
            </span>
                <div className="hidden group-hover:flex w-8 h-8 rounded-full bg-white/10 items-center justify-center hover:bg-white/20 hover:scale-105 transition-all">
                  <Play size={14} fill="white" strokeWidth={0} className="ml-0.5" />
                </div>
              </>
          )}
        </div>

        {/* Artwork */}
        <div className="relative w-11 h-11 rounded-xl overflow-hidden shrink-0 ring-1 ring-white/[0.08] shadow-md">
          {cover ? (
              <img src={cover} alt="" className="w-full h-full object-cover" loading="lazy" />
          ) : (
              <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.05] to-transparent">
                <Music size={14} className="text-white/20" />
              </div>
          )}
        </div>

        {/* Title & Artist */}
        <div className="flex-1 min-w-0 flex flex-col justify-center">
          <p
              className={`text-[14px] font-medium truncate cursor-pointer transition-colors duration-200 ${
                  isThis ? "text-accent drop-shadow-[0_0_8px_rgba(255,85,0,0.4)]" : "text-white/90 hover:text-white"
              }`}
              onClick={() => navigate(`/track/${encodeURIComponent(track.urn)}`)}
          >
            {track.title}
          </p>
          <p
              className="text-[12px] text-white/40 truncate mt-0.5 cursor-pointer hover:text-white/70 transition-colors"
              onClick={() => navigate(`/user/${encodeURIComponent(track.user.urn)}`)}
          >
            {track.user.username}
          </p>
        </div>

        {/* Stats */}
        <div className="hidden sm:flex items-center gap-4 shrink-0 pr-4">
          {track.playback_count != null && (
              <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-16">
            <Headphones size={11} className="text-white/20" />
                {fc(track.playback_count)}
          </span>
          )}
          {(track.favoritings_count ?? track.likes_count) != null && (
              <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-14">
            <Heart size={11} className="text-white/20" />
                {fc(track.favoritings_count ?? track.likes_count)}
          </span>
          )}
        </div>

        {/* Duration */}
        <span className="text-[12px] text-white/30 tabular-nums font-medium shrink-0 w-12 text-right">
        {dur(track.duration)}
      </span>
      </div>
  );
}

/* ── Playlist Card ────────────────────────────────────────── */

function UserPlaylistCard({ playlist }: { playlist: Playlist }) {
  const navigate = useNavigate();
  const { play, pause, resume, currentTrack, isPlaying } = usePlayerStore();
  const cover = art(playlist.artwork_url, "t300x300");

  const isPlayingFromThis = currentTrack
      ? playlist.tracks?.some?.((t: Track) => t.urn === currentTrack.urn)
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
    if (playlist.tracks && playlist.tracks.length > 0) {
      play(playlist.tracks[0], playlist.tracks);
    } else {
      navigate(`/playlist/${encodeURIComponent(playlist.urn)}`);
    }
  };

  return (
      <div
          className="group relative flex flex-col gap-3"
          onClick={() => navigate(`/playlist/${encodeURIComponent(playlist.urn)}`)}
      >
        <div className="relative aspect-square rounded-2xl overflow-hidden bg-white/[0.02] cursor-pointer ring-1 ring-white/[0.06] shadow-lg group-hover:shadow-2xl group-hover:ring-white/[0.15] transition-all duration-500 ease-[var(--ease-apple)]">
          {cover ? (
              <img
                  src={cover}
                  alt={playlist.title}
                  className="w-full h-full object-cover transition-transform duration-700 ease-[var(--ease-apple)] group-hover:scale-[1.05]"
                  loading="lazy"
              />
          ) : (
              <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.04] to-transparent">
                <ListMusic size={32} className="text-white/10" />
              </div>
          )}

          <div
              className={`absolute inset-0 flex items-center justify-center transition-all duration-300 ${
                  isPlayingFromThis && isPlaying
                      ? "bg-black/40 backdrop-blur-sm opacity-100"
                      : "bg-black/0 opacity-0 group-hover:bg-black/40 group-hover:backdrop-blur-sm group-hover:opacity-100"
              }`}
          >
            <div
                onClick={handlePlay}
                className={`w-14 h-14 rounded-full flex items-center justify-center transition-all duration-300 ease-[var(--ease-apple)] shadow-2xl hover:scale-110 active:scale-95 ${
                    isPlayingFromThis && isPlaying
                        ? "bg-white scale-100"
                        : "bg-white/90 scale-75 group-hover:scale-100"
                }`}
            >
              {isPlayingFromThis && isPlaying ? (
                  <Pause size={22} fill="black" strokeWidth={0} />
              ) : (
                  <Play size={22} fill="black" strokeWidth={0} className="ml-1" />
              )}
            </div>
          </div>

          {playlist.track_count != null && (
              <div className="absolute bottom-2.5 right-2.5 flex items-center gap-1.5 text-[11px] font-medium bg-black/60 backdrop-blur-md text-white/90 px-2.5 py-1 rounded-full opacity-0 group-hover:opacity-100 transition-opacity duration-300 shadow-lg">
                <ListMusic size={11} />
                {playlist.track_count}
              </div>
          )}
        </div>

        <div className="min-w-0 px-1">
          <p className="text-[14px] font-semibold text-white/90 truncate leading-snug cursor-pointer group-hover:text-white transition-colors duration-200">
            {playlist.title}
          </p>
          <div className="flex items-center gap-2 mt-1">
          <span className="text-[10px] font-bold text-white/30 uppercase tracking-wider bg-white/[0.05] px-1.5 py-0.5 rounded-md">
            {playlist.playlist_type || "Playlist"}
          </span>
            {playlist.likes_count > 0 && (
                <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1">
              <Heart size={10} className="text-white/20" />
                  {fc(playlist.likes_count)}
            </span>
            )}
          </div>
        </div>
      </div>
  );
}

/* ── Main: UserPage ──────────────────────────────────────── */

export function UserPage() {
  const { urn } = useParams<{ urn: string }>();
  const { t } = useTranslation();
  const currentUser = useAuthStore((s) => s.user);

  const[activeTab, setActiveTab] = useState<"tracks" | "playlists" | "likes">("tracks");

  // Profile data
  const { data: user, isLoading: userLoading } = useUser(urn);
  const { data: webProfiles } = useUserWebProfiles(urn);

  // Tab data
  const tracksQuery = useUserTracks(urn);
  const playlistsQuery = useUserPlaylists(urn);
  const likesQuery = useUserLikedTracks(urn);

  // Infinite scroll
  const hasNextPage =
      activeTab === "tracks" ? tracksQuery.hasNextPage :
          activeTab === "playlists" ? playlistsQuery.hasNextPage :
              likesQuery.hasNextPage;

  const isFetchingNextPage =
      activeTab === "tracks" ? tracksQuery.isFetchingNextPage :
          activeTab === "playlists" ? playlistsQuery.isFetchingNextPage :
              likesQuery.isFetchingNextPage;

  const fetchNextPage =
      activeTab === "tracks" ? tracksQuery.fetchNextPage :
          activeTab === "playlists" ? playlistsQuery.fetchNextPage :
              likesQuery.fetchNextPage;

  const sentinelRef = useInfiniteScroll(
      !!hasNextPage,
      !!isFetchingNextPage,
      fetchNextPage
  );

  if (userLoading || !user) {
    return (
        <div className="h-full flex items-center justify-center">
          <Loader2 size={28} className="text-accent animate-spin" />
        </div>
    );
  }

  const avatar = art(user.avatar_url, "t500x500");
  const isOwnProfile = currentUser?.urn === user.urn;

  const tabs =[
    { id: "tracks", label: t("user.tracks"), count: user.track_count },
    { id: "playlists", label: t("user.playlists"), count: user.playlist_count },
    { id: "likes", label: t("user.likes"), count: user.public_favorites_count },
  ] as const;

  const renderTabContent = () => {
    if (activeTab === "tracks") {
      if (tracksQuery.isLoading) return <div className="py-12 flex justify-center"><Loader2 size={24} className="animate-spin text-white/20" /></div>;

      const uniqueTracks = Array.from(new Map(tracksQuery.tracks.map(t =>[t.urn, t])).values());
      if (uniqueTracks.length === 0) return <div className="py-12 text-center text-white/30 text-sm">No tracks found.</div>;

      return (
          <div className="flex flex-col gap-1">
            {uniqueTracks.map((track, i) => (
                <TrackRow key={`${track.urn}-${i}`} track={track} index={i} queue={uniqueTracks} />
            ))}
          </div>
      );
    }

    if (activeTab === "playlists") {
      if (playlistsQuery.isLoading) return <div className="py-12 flex justify-center"><Loader2 size={24} className="animate-spin text-white/20" /></div>;

      const uniquePlaylists = Array.from(new Map(playlistsQuery.playlists.map(p => [p.urn, p])).values());
      if (uniquePlaylists.length === 0) return <div className="py-12 text-center text-white/30 text-sm">No playlists found.</div>;

      return (
          <div className="grid grid-cols-[repeat(auto-fill,minmax(180px,1fr))] gap-6">
            {uniquePlaylists.map((playlist, i) => (
                <UserPlaylistCard key={`${playlist.urn}-${i}`} playlist={playlist} />
            ))}
          </div>
      );
    }

    if (activeTab === "likes") {
      if (likesQuery.isLoading) return <div className="py-12 flex justify-center"><Loader2 size={24} className="animate-spin text-white/20" /></div>;

      const uniqueLikes = Array.from(new Map(likesQuery.tracks.map(t =>[t.urn, t])).values());
      if (uniqueLikes.length === 0) return <div className="py-12 text-center text-white/30 text-sm">No liked tracks.</div>;

      return (
          <div className="flex flex-col gap-1">
            {uniqueLikes.map((track, i) => (
                <TrackRow key={`${track.urn}-${i}`} track={track} index={i} queue={uniqueLikes} />
            ))}
          </div>
      );
    }
  };

  return (
      <div className="p-6 pb-4 space-y-8 animate-fade-in-up">

        {/* ── Public Profile Warning ── */}
        {isOwnProfile && (
            <div className="bg-gradient-to-r from-amber-500/10 to-amber-500/5 border border-amber-500/20 text-amber-400/90 px-5 py-3.5 rounded-2xl flex items-center gap-3 text-[13px] font-medium backdrop-blur-xl shadow-lg">
              <AlertCircle size={18} />
              {t("user.publicProfile")}
            </div>
        )}

        {/* ── Hero Section (Vision Pro Style) ── */}
        <section className="relative rounded-[32px] overflow-hidden bg-white/[0.02] border border-white/[0.05] shadow-2xl">
          {/* Deep blur background */}
          {avatar && (
              <div className="absolute inset-0 pointer-events-none overflow-hidden">
                <img
                    src={avatar}
                    alt=""
                    className="w-full h-full object-cover scale-[2] blur-[100px] opacity-30 saturate-200"
                />
                <div className="absolute inset-0 bg-gradient-to-b from-[rgb(8,8,10)]/50 via-[rgb(8,8,10)]/40 to-[rgb(8,8,10)]/90" />
              </div>
          )}

          <div className="relative flex flex-col md:flex-row items-center md:items-end gap-8 p-8 md:p-10">
            {/* Avatar */}
            <div className="w-[180px] h-[180px] md:w-[200px] md:h-[200px] rounded-full overflow-hidden shrink-0 shadow-[0_0_60px_rgba(0,0,0,0.6)] ring-2 ring-white/[0.15] bg-black/40 relative group">
              {avatar ? (
                  <img src={avatar} alt={user.username} className="w-full h-full object-cover transition-transform duration-700 group-hover:scale-105" />
              ) : (
                  <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.05] to-transparent">
                    <Users size={64} className="text-white/20" />
                  </div>
              )}
            </div>

            {/* User Info */}
            <div className="flex-1 min-w-0 flex flex-col items-center md:items-start text-center md:text-left">
              {user.plan && user.plan !== "Free" && (
                  <span className="inline-block text-[10px] font-extrabold px-3 py-1 rounded-full bg-gradient-to-r from-accent to-accent-hover text-white shadow-[0_0_20px_var(--color-accent-glow)] mb-4 uppercase tracking-widest">
                {user.plan}
              </span>
              )}

              <h1 className="text-4xl md:text-5xl font-extrabold text-white tracking-tight leading-tight mb-2 drop-shadow-xl">
                {user.username}
              </h1>

              {(user.full_name || user.city || user.country) && (
                  <p className="text-[15px] text-white/60 mb-6 flex flex-col md:flex-row items-center gap-2 md:gap-4 font-medium">
                    {user.full_name && <span>{user.full_name}</span>}
                    {user.full_name && (user.city || user.country) && <span className="hidden md:inline text-white/20">•</span>}
                    {(user.city || user.country) && (
                        <span className="flex items-center gap-1.5">
                    <MapPin size={14} className="text-white/40" />
                          {[user.city, user.country].filter(Boolean).join(", ")}
                  </span>
                    )}
                  </p>
              )}

              <div className="flex items-center flex-wrap justify-center md:justify-start gap-8 mt-auto w-full">
                {user.followers_count != null && (
                    <div className="flex flex-col">
                      <span className="text-2xl font-bold text-white/90 tabular-nums">{fc(user.followers_count)}</span>
                      <span className="text-[11px] text-white/40 uppercase tracking-widest mt-1 font-semibold">{t("user.followers")}</span>
                    </div>
                )}
                {user.followings_count != null && (
                    <div className="flex flex-col">
                      <span className="text-2xl font-bold text-white/90 tabular-nums">{fc(user.followings_count)}</span>
                      <span className="text-[11px] text-white/40 uppercase tracking-widest mt-1 font-semibold">{t("user.following")}</span>
                    </div>
                )}

                {!isOwnProfile && (
                    <div className="ml-auto">
                      <FollowBtn userUrn={user.urn} />
                    </div>
                )}
              </div>
            </div>
          </div>
        </section>

        {/* ── Two Column Layout ── */}
        <div className="grid grid-cols-1 lg:grid-cols-[1fr_320px] gap-8 items-start">

          {/* Left Column (Main Content) */}
          <div className="min-w-0 flex flex-col gap-6">

            {/* Apple-style Segmented Control for Tabs */}
            <div className="flex items-center gap-1.5 p-1.5 bg-white/[0.02] border border-white/[0.05] rounded-2xl w-fit backdrop-blur-2xl shadow-lg">
              {tabs.map((tab) => {
                const isActive = activeTab === tab.id;
                return (
                    <button
                        key={tab.id}
                        onClick={() => setActiveTab(tab.id)}
                        className={`flex items-center gap-2 px-6 py-2.5 rounded-xl text-[13px] font-semibold transition-all duration-300 ease-[var(--ease-apple)] ${
                            isActive
                                ? "bg-white/[0.12] text-white shadow-md border border-white/[0.05]"
                                : "text-white/40 hover:text-white/80 hover:bg-white/[0.04] border border-transparent cursor-pointer"
                        }`}
                    >
                      {tab.label}
                      {tab.count != null && (
                          <span className={`text-[11px] tabular-nums px-2 py-0.5 rounded-full transition-colors ${
                              isActive ? "bg-white/20 text-white" : "bg-white/5 text-white/30"
                          }`}>
                      {fc(tab.count)}
                    </span>
                      )}
                    </button>
                );
              })}
            </div>

            {/* Grid/List Content */}
            <div className="min-h-[400px]">
              {renderTabContent()}

              {/* Infinite Scroll Sentinel */}
              <div ref={sentinelRef} className="h-16 flex items-center justify-center mt-6">
                {isFetchingNextPage && (
                    <Loader2 size={24} className="text-white/20 animate-spin" />
                )}
              </div>
            </div>
          </div>

          {/* Right Column (Sidebar) */}
          <div className="space-y-5 lg:sticky lg:top-6">

            {/* Bio / Description */}
            {user.description && (
                <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl">
                  <h3 className="text-[14px] font-bold text-white/60 mb-4 tracking-tight">{t("user.about")}</h3>
                  <p className="text-[13px] text-white/50 leading-relaxed whitespace-pre-wrap break-words">
                    {user.description}
                  </p>
                </section>
            )}

            {/* Stats & Joined */}
            <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl flex flex-col gap-4">
              <div className="flex items-center justify-between text-[13px]">
                <span className="text-white/40 font-medium">{t("user.memberSince")}</span>
                <span className="text-white/80 font-semibold flex items-center gap-2">
                <Calendar size={14} className="text-white/30" />
                  {dateFormatted(user.created_at)}
              </span>
              </div>
            </section>

            {/* Web Profiles (Social Links) */}
            {webProfiles && webProfiles.length > 0 && (
                <section className="bg-white/[0.02] border border-white/[0.05] backdrop-blur-[60px] rounded-3xl p-6 shadow-xl">
                  <h3 className="text-[14px] font-bold text-white/60 mb-4 tracking-tight">{t("user.links")}</h3>
                  <div className="flex flex-col gap-1.5">
                    {webProfiles.map((link) => (
                        <a
                            key={link.id}
                            href={link.url}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="flex items-center gap-3 p-3 rounded-2xl hover:bg-white/[0.06] transition-all duration-200 group/link border border-transparent hover:border-white/[0.04]"
                        >
                          <div className="w-10 h-10 rounded-full bg-white/[0.04] flex items-center justify-center text-white/40 group-hover/link:text-white group-hover/link:bg-white/[0.1] group-hover:scale-105 transition-all duration-300 shadow-sm">
                            {getWebIcon(link.service)}
                          </div>
                          <div className="flex-1 min-w-0">
                            <p className="text-[13px] text-white/80 font-semibold truncate group-hover/link:text-white transition-colors">
                              {link.title}
                            </p>
                            {link.username && (
                                <p className="text-[11px] text-white/30 truncate mt-0.5 font-medium">
                                  @{link.username}
                                </p>
                            )}
                          </div>
                        </a>
                    ))}
                  </div>
                </section>
            )}
          </div>
        </div>
      </div>
  );
}