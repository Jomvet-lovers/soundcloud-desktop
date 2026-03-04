import { useState, useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";
import { useTranslation } from "react-i18next";
import {
    Search as SearchIcon,
    X,
    Play,
    Pause,
    Music,
    ListMusic,
    Users,
    Loader2,
    Headphones,
    Heart
} from "lucide-react";
import {
    useSearchTracks,
    useSearchPlaylists,
    useSearchUsers,
    useInfiniteScroll,
    type Playlist,
    type SCUser
} from "../lib/hooks";
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

/* ── Components ───────────────────────────────────────────── */

function TrackRow({ track, index, queue }: { track: Track; index: number; queue: Track[] }) {
    const { play, pause, resume, currentTrack, isPlaying } = usePlayerStore();
    const navigate = useNavigate();
    const isThis = currentTrack?.urn === track.urn;
    const cover = art(track.artwork_url, "t200x200");

    const handlePlay = (e: React.MouseEvent) => {
        e.stopPropagation();
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
            <div
                className="w-10 h-10 flex items-center justify-center shrink-0 cursor-pointer"
                onClick={handlePlay}
            >
                {isThis && isPlaying ? (
                    <div className="w-9 h-9 rounded-full bg-accent flex items-center justify-center shadow-[0_0_15px_var(--color-accent-glow)] scale-100 animate-fade-in-up">
                        <Pause size={16} fill="white" strokeWidth={0} />
                    </div>
                ) : (
                    <div className="w-9 h-9 rounded-full bg-white/[0.06] group-hover:bg-white/10 flex items-center justify-center transition-all">
                        <Play size={16} fill="white" strokeWidth={0} className="ml-0.5 opacity-60 group-hover:opacity-100" />
                    </div>
                )}
            </div>

            <div className="relative w-12 h-12 rounded-xl overflow-hidden shrink-0 ring-1 ring-white/[0.08] shadow-md">
                {cover ? (
                    <img src={cover} alt="" className="w-full h-full object-cover" loading="lazy" />
                ) : (
                    <div className="w-full h-full flex items-center justify-center bg-gradient-to-br from-white/[0.05] to-transparent">
                        <Music size={16} className="text-white/20" />
                    </div>
                )}
            </div>

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

            <div className="hidden md:flex items-center gap-4 shrink-0 pr-4">
                {track.playback_count != null && (
                    <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-16">
            <Headphones size={11} className="text-white/20" />
                        {fc(track.playback_count)}
          </span>
                )}
                <span className="text-[11px] text-white/30 tabular-nums flex items-center gap-1.5 w-14">
          <Heart size={11} className="text-white/20" />
                    {fc(track.favoritings_count ?? track.likes_count)}
        </span>
            </div>

            <span className="text-[12px] text-white/30 tabular-nums font-medium shrink-0 w-12 text-right">
        {dur(track.duration)}
      </span>
        </div>
    );
}

function PlaylistCard({ playlist }: { playlist: Playlist }) {
    const navigate = useNavigate();
    const cover = art(playlist.artwork_url, "t300x300");

    return (
        <div
            className="group relative flex flex-col gap-3 cursor-pointer"
            onClick={() => navigate(`/playlist/${encodeURIComponent(playlist.urn)}`)}
        >
            <div className="relative aspect-square rounded-2xl overflow-hidden bg-white/[0.02] ring-1 ring-white/[0.06] shadow-lg group-hover:shadow-2xl group-hover:ring-white/[0.15] transition-all duration-500 ease-[var(--ease-apple)]">
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

                <div className="absolute inset-0 bg-black/20 opacity-0 group-hover:opacity-100 transition-opacity duration-300" />

                {playlist.track_count != null && (
                    <div className="absolute bottom-2.5 right-2.5 flex items-center gap-1.5 text-[11px] font-medium bg-black/60 backdrop-blur-md text-white/90 px-2.5 py-1 rounded-full shadow-lg">
                        <ListMusic size={11} />
                        {playlist.track_count}
                    </div>
                )}
            </div>

            <div className="min-w-0 px-1">
                <p className="text-[14px] font-semibold text-white/90 truncate leading-snug group-hover:text-white transition-colors">
                    {playlist.title}
                </p>
                <p className="text-[12px] text-white/40 truncate mt-1">
                    {playlist.user?.username || "Unknown"}
                </p>
            </div>
        </div>
    );
}

function UserCard({ user }: { user: SCUser }) {
    const navigate = useNavigate();
    const avatar = art(user.avatar_url, "t300x300");

    return (
        <div
            className="group flex flex-col items-center gap-4 p-5 rounded-3xl bg-white/[0.02] hover:bg-white/[0.04] border border-white/[0.04] hover:border-white/[0.08] transition-all duration-300 cursor-pointer"
            onClick={() => navigate(`/user/${encodeURIComponent(user.urn)}`)}
        >
            <div className="relative w-24 h-24 rounded-full shadow-xl overflow-hidden ring-2 ring-white/[0.05] group-hover:ring-white/[0.15] group-hover:scale-105 transition-all duration-500">
                {avatar ? (
                    <img src={avatar} alt={user.username} className="w-full h-full object-cover" loading="lazy" />
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
}

/* ── Search Page ──────────────────────────────────────────── */

export function Search() {
    const { t } = useTranslation();
    const [inputValue, setInputValue] = useState("");
    const [debouncedQuery, setDebouncedQuery] = useState("");
    const [activeTab, setActiveTab] = useState<"tracks" | "playlists" | "users">("tracks");

    // Debounce logic
    useEffect(() => {
        const handler = setTimeout(() => {
            setDebouncedQuery(inputValue);
        }, 500);
        return () => clearTimeout(handler);
    }, [inputValue]);

    // Queries
    const tracksQuery = useSearchTracks(debouncedQuery);
    const playlistsQuery = useSearchPlaylists(debouncedQuery);
    const usersQuery = useSearchUsers(debouncedQuery);

    // Determine active query for infinite scroll
    const activeQuery =
        activeTab === "tracks" ? tracksQuery :
            activeTab === "playlists" ? playlistsQuery :
                usersQuery;

    const sentinelRef = useInfiniteScroll(
        !!activeQuery.hasNextPage,
        !!activeQuery.isFetchingNextPage,
        activeQuery.fetchNextPage
    );

    const tabs = [
        { id: "tracks", label: t("search.tracks") },
        { id: "playlists", label: t("search.playlists") },
        { id: "users", label: t("search.users") },
    ] as const;

    const renderContent = () => {
        if (!debouncedQuery) {
            return (
                <div className="flex flex-col items-center justify-center h-[400px] text-white/20">
                    <SearchIcon size={48} className="mb-4 opacity-50" />
                    <p className="text-sm font-medium">Search for artists, bands, tracks, podcasts</p>
                </div>
            );
        }

        if (activeTab === "tracks") {
            const uniqueTracks = Array.from(new Map(tracksQuery.tracks.map(t => [t.urn, t])).values());

            if (tracksQuery.isLoading) return <div className="flex justify-center py-20"><Loader2 size={32} className="animate-spin text-white/20" /></div>;
            if (uniqueTracks.length === 0) return <div className="py-20 text-center text-white/30">{t("search.noResults")}</div>;

            return (
                <div className="flex flex-col gap-1">
                    {uniqueTracks.map((track, i) => (
                        <TrackRow key={`${track.urn}-${i}`} track={track} index={i} queue={uniqueTracks} />
                    ))}
                </div>
            );
        }

        if (activeTab === "playlists") {
            const uniquePlaylists = Array.from(new Map(playlistsQuery.playlists.map(p => [p.urn, p])).values());

            if (playlistsQuery.isLoading) return <div className="flex justify-center py-20"><Loader2 size={32} className="animate-spin text-white/20" /></div>;
            if (uniquePlaylists.length === 0) return <div className="py-20 text-center text-white/30">{t("search.noResults")}</div>;

            return (
                <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-6">
                    {uniquePlaylists.map((p, i) => (
                        <PlaylistCard key={`${p.urn}-${i}`} playlist={p} />
                    ))}
                </div>
            );
        }

        if (activeTab === "users") {
            const uniqueUsers = Array.from(new Map(usersQuery.users.map(u => [u.urn, u])).values());

            if (usersQuery.isLoading) return <div className="flex justify-center py-20"><Loader2 size={32} className="animate-spin text-white/20" /></div>;
            if (uniqueUsers.length === 0) return <div className="py-20 text-center text-white/30">{t("search.noResults")}</div>;

            return (
                <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-4">
                    {uniqueUsers.map((u, i) => (
                        <UserCard key={`${u.urn}-${i}`} user={u} />
                    ))}
                </div>
            );
        }
    };

    return (
        <div className="p-6 pb-4 space-y-8 animate-fade-in-up">
            {/* ── Search Input ── */}
            <div className="relative max-w-2xl mx-auto">
                <div className="absolute inset-y-0 left-4 flex items-center pointer-events-none">
                    <SearchIcon size={20} className="text-white/40" />
                </div>
                <input
                    type="text"
                    value={inputValue}
                    onChange={(e) => setInputValue(e.target.value)}
                    placeholder={t("search.placeholder")}
                    className="w-full bg-white/[0.04] hover:bg-white/[0.06] focus:bg-white/[0.08] text-white placeholder:text-white/30 text-[16px] py-4 pl-12 pr-12 rounded-[20px] outline-none border border-white/[0.05] focus:border-accent/30 focus:ring-1 focus:ring-accent/30 transition-all duration-300 shadow-xl backdrop-blur-md"
                    autoFocus
                />
                {inputValue && (
                    <button
                        onClick={() => setInputValue("")}
                        className="absolute inset-y-0 right-4 flex items-center text-white/30 hover:text-white cursor-pointer transition-colors"
                    >
                        <X size={18} />
                    </button>
                )}
            </div>

            {/* ── Tabs ── */}
            {debouncedQuery && (
                <div className="flex items-center justify-center gap-1.5 p-1.5 bg-white/[0.02] border border-white/[0.05] rounded-2xl w-fit backdrop-blur-2xl shadow-lg mx-auto">
                    {tabs.map((tab) => {
                        const isActive = activeTab === tab.id;
                        return (
                            <button
                                key={tab.id}
                                onClick={() => setActiveTab(tab.id as any)}
                                className={`flex items-center gap-2 px-6 py-2.5 rounded-xl text-[13px] font-semibold transition-all duration-300 ease-[var(--ease-apple)] ${
                                    isActive
                                        ? "bg-white/[0.12] text-white shadow-md border border-white/[0.05]"
                                        : "text-white/40 hover:text-white/80 hover:bg-white/[0.04] border border-transparent"
                                }`}
                            >
                                {tab.label}
                            </button>
                        );
                    })}
                </div>
            )}

            {/* ── Content ── */}
            <div className="min-h-[400px]">
                {renderContent()}

                {/* Sentinel */}
                <div ref={sentinelRef} className="h-20 flex items-center justify-center mt-6">
                    {activeQuery.isFetchingNextPage && (
                        <Loader2 size={24} className="text-white/20 animate-spin" />
                    )}
                </div>
            </div>
        </div>
    );
}