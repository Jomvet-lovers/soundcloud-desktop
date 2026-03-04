import { Play, Pause } from "lucide-react";
import { useNavigate } from "react-router-dom";
import type { Track } from "../../stores/player";
import { usePlayerStore } from "../../stores/player";
import { preloadTrack } from "../../lib/audio";

interface TrackCardProps {
  track: Track;
  queue?: Track[];
}

function formatDuration(ms: number) {
  const totalSec = Math.floor(ms / 1000);
  const m = Math.floor(totalSec / 60);
  const s = totalSec % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function formatCount(n?: number) {
  if (!n) return "0";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toString();
}

export function TrackCard({ track, queue }: TrackCardProps) {
  const navigate = useNavigate();
  const { currentTrack, isPlaying, play, pause, resume } = usePlayerStore();
  const isThis = currentTrack?.urn === track.urn;
  const artwork = track.artwork_url?.replace("-large", "-t300x300");

  const handlePlay = (e: React.MouseEvent) => {
    e.stopPropagation();
    if (isThis && isPlaying) pause();
    else if (isThis) resume();
    else play(track, queue);
  };

  return (
    <div
      className="group relative"
      onMouseEnter={() => preloadTrack(track.urn)}
    >
      {/* Artwork */}
      <div
        className="relative aspect-square rounded-2xl overflow-hidden bg-white/[0.03] cursor-pointer ring-1 ring-white/[0.06] group-hover:ring-white/[0.12] transition-all duration-300 ease-[var(--ease-apple)]"
        onClick={handlePlay}
      >
        {artwork ? (
          <img
            src={artwork}
            alt={track.title}
            className="w-full h-full object-cover transition-transform duration-500 ease-[var(--ease-apple)] group-hover:scale-[1.04]"
            loading="lazy"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center text-white/20">
            <Play size={32} />
          </div>
        )}

        {/* Hover overlay */}
        <div
          className={`absolute inset-0 flex items-center justify-center transition-all duration-300 ${
            isThis && isPlaying
              ? "bg-black/30 backdrop-blur-[2px] opacity-100"
              : "bg-black/0 opacity-0 group-hover:bg-black/30 group-hover:backdrop-blur-[2px] group-hover:opacity-100"
          }`}
        >
          <div className={`w-12 h-12 rounded-full flex items-center justify-center transition-all duration-300 ease-[var(--ease-apple)] shadow-xl ${
            isThis && isPlaying
              ? "bg-white scale-100"
              : "bg-white/90 scale-75 group-hover:scale-100"
          }`}>
            {isThis && isPlaying ? (
              <Pause size={20} fill="black" strokeWidth={0} />
            ) : (
              <Play size={20} fill="black" strokeWidth={0} className="ml-0.5" />
            )}
          </div>
        </div>

        {/* Duration pill */}
        <div className="absolute bottom-2 right-2 text-[10px] font-medium bg-black/50 backdrop-blur-md text-white/80 px-2 py-0.5 rounded-full opacity-0 group-hover:opacity-100 transition-opacity duration-200">
          {formatDuration(track.duration)}
        </div>
      </div>

      {/* Info */}
      <div className="mt-3 min-w-0">
        <p
          className="text-[13px] font-medium text-white/90 truncate leading-snug cursor-pointer hover:text-white transition-colors duration-150"
          onClick={() => navigate(`/track/${encodeURIComponent(track.urn)}`)}
        >
          {track.title}
        </p>
        <p
          className="text-[11px] text-white/35 truncate mt-0.5 cursor-pointer hover:text-white/55 transition-colors duration-150"
          onClick={() => navigate(`/user/${encodeURIComponent(track.user.urn)}`)}
        >
          {track.user.username}
        </p>
        {track.playback_count != null && (
          <p className="text-[10px] text-white/20 mt-1 tabular-nums">
            {formatCount(track.playback_count)} plays
          </p>
        )}
      </div>
    </div>
  );
}
