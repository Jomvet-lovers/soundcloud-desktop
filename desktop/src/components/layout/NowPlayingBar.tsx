import { useRef, useState, useCallback } from "react";
import {
  Play,
  Pause,
  SkipBack,
  SkipForward,
  Volume2,
  VolumeX,
  Volume1,
  Heart,
  Shuffle,
  Repeat,
  Repeat1,
  ListMusic,
} from "lucide-react";
import { useNavigate } from "react-router-dom";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { usePlayerStore, type Track } from "../../stores/player";
import { api } from "../../lib/api";

function formatTime(seconds: number) {
  if (!seconds || !isFinite(seconds)) return "0:00";
  const m = Math.floor(seconds / 60);
  const s = Math.floor(seconds % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

/* ── Progress Slider (full-width, two-tone) ──────────────────────── */
function ProgressSlider({
  value,
  max,
  onChange,
}: {
  value: number;
  max: number;
  onChange: (v: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);
  const [hoverRatio, setHoverRatio] = useState<number | null>(null);

  const ratio = max > 0 ? Math.min(value / max, 1) : 0;
  const activeRatio = dragging && hoverRatio !== null ? hoverRatio : ratio;
  const previewRatio = hoverRatio;

  const calcRatio = useCallback((clientX: number) => {
    if (!ref.current) return 0;
    const rect = ref.current.getBoundingClientRect();
    return Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
  }, []);

  const handlePointerDown = (e: React.PointerEvent) => {
    e.preventDefault();
    setDragging(true);
    (e.target as HTMLElement).setPointerCapture(e.pointerId);
    onChange(calcRatio(e.clientX) * max);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    const r = calcRatio(e.clientX);
    setHoverRatio(r);
    if (dragging) onChange(r * max);
  };

  const handlePointerUp = () => setDragging(false);
  const handlePointerLeave = () => {
    if (!dragging) setHoverRatio(null);
  };

  return (
    <div
      ref={ref}
      className="relative h-5 flex items-center cursor-pointer group"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerLeave={handlePointerLeave}
    >
      {/* Track bg */}
      <div className="absolute inset-x-0 h-[3px] rounded-full bg-white/[0.08] group-hover:h-[5px] transition-all duration-150">
        {/* Hover preview zone (lighter, behind active) */}
        {previewRatio !== null && !dragging && previewRatio > ratio && (
          <div
            className="absolute top-0 h-full rounded-full bg-white/[0.08] transition-[width] duration-75"
            style={{ left: `${ratio * 100}%`, width: `${(previewRatio - ratio) * 100}%` }}
          />
        )}
        {/* Active fill */}
        <div
          className="h-full rounded-full bg-accent transition-[width] duration-75"
          style={{ width: `${activeRatio * 100}%` }}
        />
      </div>

      {/* Thumb */}
      <div
        className={`absolute top-1/2 -translate-y-1/2 -translate-x-1/2 rounded-full transition-all duration-150 ${
          dragging
            ? "w-4 h-4 scale-100 opacity-100 bg-accent shadow-[0_0_12px_var(--color-accent-glow)]"
            : "w-3 h-3 scale-0 opacity-0 group-hover:scale-100 group-hover:opacity-100 bg-accent shadow-[0_0_10px_var(--color-accent-glow)]"
        }`}
        style={{ left: `${activeRatio * 100}%` }}
      />
    </div>
  );
}

/* ── Volume Slider (0-200%, extra zone after 100%) ──────────────── */
function VolumeSlider({
  volume,
  onChange,
  className = "",
}: {
  volume: number; // 0-200
  onChange: (v: number) => void;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [dragging, setDragging] = useState(false);

  const ratio = volume / 200; // 0-1
  const midpoint = 0.5; // 100% mark

  const calcVolume = useCallback((clientX: number) => {
    if (!ref.current) return 0;
    const rect = ref.current.getBoundingClientRect();
    const r = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return Math.round(r * 200);
  }, []);

  return (
    <div
      ref={ref}
      className={`relative h-5 flex items-center cursor-pointer group ${className}`}
      onPointerDown={(e) => {
        e.preventDefault();
        setDragging(true);
        (e.target as HTMLElement).setPointerCapture(e.pointerId);
        onChange(calcVolume(e.clientX));
      }}
      onPointerMove={(e) => { if (dragging) onChange(calcVolume(e.clientX)); }}
      onPointerUp={() => setDragging(false)}
      onPointerLeave={() => setDragging(false)}
      onWheel={(e) => { e.preventDefault(); onChange(Math.max(0, Math.min(200, volume + (e.deltaY < 0 ? 1 : -1)))); }}
    >
      {/* Track bg */}
      <div className="absolute inset-x-0 h-[3px] rounded-full bg-white/[0.08] group-hover:h-[4px] transition-all duration-150">
        {/* Normal fill (white) up to min(ratio, midpoint) */}
        <div
          className="absolute top-0 left-0 h-full rounded-full bg-white/60"
          style={{ width: `${Math.min(ratio, midpoint) * 100}%` }}
        />
        {/* Extra fill (amber/accent) from midpoint to ratio */}
        {ratio > midpoint && (
          <div
            className="absolute top-0 h-full rounded-r-full bg-amber-400/80"
            style={{ left: `${midpoint * 100}%`, width: `${(ratio - midpoint) * 100}%` }}
          />
        )}
        {/* 100% tick mark */}
        <div
          className="absolute top-0 h-full w-px bg-white/20"
          style={{ left: `${midpoint * 100}%` }}
        />
      </div>
      {/* Thumb */}
      <div
        className={`absolute top-1/2 -translate-y-1/2 -translate-x-1/2 w-2.5 h-2.5 rounded-full transition-all duration-150 ${
          ratio > midpoint ? "bg-amber-400" : "bg-white"
        } ${
          dragging ? "scale-100 opacity-100" : "scale-0 opacity-0 group-hover:scale-100 group-hover:opacity-100"
        }`}
        style={{ left: `${ratio * 100}%` }}
      />
    </div>
  );
}

/* ── Control button ──────────────────────────────────────────────── */
function ControlBtn({
  onClick,
  active = false,
  children,
  size = "default",
}: {
  onClick: () => void;
  active?: boolean;
  children: React.ReactNode;
  size?: "default" | "sm";
}) {
  const s = size === "sm" ? "w-9 h-9" : "w-10 h-10";
  return (
    <button
      type="button"
      onClick={onClick}
      className={`${s} rounded-full flex items-center justify-center transition-all duration-150 ease-[var(--ease-apple)] cursor-pointer hover:bg-white/[0.04] ${
        active ? "text-accent" : "text-white/40 hover:text-white/70"
      }`}
    >
      {children}
    </button>
  );
}

/* ── Like button ─────────────────────────────────────────────────── */
function LikeButton({ trackUrn }: { trackUrn: string }) {
  const qc = useQueryClient();

  // Fetch actual user_favorite state from API
  const { data: trackData } = useQuery({
    queryKey: ["track", trackUrn],
    queryFn: () => api<Track>(`/tracks/${encodeURIComponent(trackUrn)}`),
    enabled: !!trackUrn,
    staleTime: 30_000,
  });

  const [liked, setLiked] = useState<boolean | null>(null);
  const prevUrn = useRef(trackUrn);

  // Reset override when track changes
  if (prevUrn.current !== trackUrn) {
    prevUrn.current = trackUrn;
    setLiked(null);
  }

  const isLiked = liked ?? trackData?.user_favorite ?? false;

  const toggle = async () => {
    const next = !isLiked;
    setLiked(next);
    try {
      await api(`/likes/tracks/${encodeURIComponent(trackUrn)}`, {
        method: next ? "POST" : "DELETE",
      });
      qc.invalidateQueries({ queryKey: ["track", trackUrn], exact: true });
      qc.invalidateQueries({ queryKey: ["track", trackUrn, "favoriters"] });
    } catch {
      setLiked(!next);
    }
  };

  return (
    <button
      type="button"
      onClick={toggle}
      className={`w-9 h-9 rounded-full flex items-center justify-center shrink-0 transition-all duration-200 cursor-pointer hover:bg-white/[0.04] ${
        isLiked ? "text-accent" : "text-white/30 hover:text-white/60"
      }`}
    >
      <Heart size={16} fill={isLiked ? "currentColor" : "none"} />
    </button>
  );
}

/* ── NowPlayingBar ───────────────────────────────────────────────── */
export function NowPlayingBar({ onQueueToggle, queueOpen }: { onQueueToggle: () => void; queueOpen: boolean }) {
  const navigate = useNavigate();
  const {
    currentTrack,
    isPlaying,
    progress,
    duration,
    volume,
    shuffle,
    repeat,
    togglePlay,
    next,
    prev,
    seek,
    setVolume,
    toggleShuffle,
    toggleRepeat,
  } = usePlayerStore();

  const artwork = currentTrack?.artwork_url?.replace("-large", "-t200x200");

  return (
    <div className="shrink-0 relative">
      {/* Glow from artwork */}
      {artwork && (
        <div
          className="absolute inset-0 opacity-[0.05] blur-3xl pointer-events-none"
          style={{ backgroundImage: `url(${artwork})`, backgroundSize: "cover", backgroundPosition: "center" }}
        />
      )}

      {/* Progress bar — full width on top */}
      <ProgressSlider value={progress} max={duration} onChange={seek} />

      <div className="h-[76px] flex items-center px-5 gap-3 relative">
        {/* ── Left: track info ── */}
        <div className="flex items-center gap-3.5 w-[280px] min-w-0">
          {currentTrack ? (
            <>
              <div
                className="w-14 h-14 rounded-[10px] shrink-0 overflow-hidden cursor-pointer shadow-xl shadow-black/40 ring-1 ring-white/[0.06] hover:ring-white/[0.12] transition-all duration-200"
                onClick={() => navigate(`/track/${encodeURIComponent(currentTrack.urn)}`)}
              >
                {artwork ? (
                  <img src={artwork} alt="" className="w-full h-full object-cover" />
                ) : (
                  <div className="w-full h-full bg-white/[0.04]" />
                )}
              </div>
              <div className="min-w-0 flex-1">
                <p
                  className="text-[13px] text-white/90 truncate font-medium cursor-pointer hover:text-white leading-tight transition-colors"
                  onClick={() => navigate(`/track/${encodeURIComponent(currentTrack.urn)}`)}
                >
                  {currentTrack.title}
                </p>
                <p
                  className="text-[11px] text-white/35 truncate mt-1 cursor-pointer hover:text-white/55 transition-colors"
                  onClick={() => navigate(`/user/${encodeURIComponent(currentTrack.user.urn)}`)}
                >
                  {currentTrack.user.username}
                </p>
              </div>
              <LikeButton trackUrn={currentTrack.urn} />
            </>
          ) : (
            <p className="text-[13px] text-white/15">Not playing</p>
          )}
        </div>

        {/* ── Center: controls ── */}
        <div className="flex-1 flex flex-col items-center gap-0.5">
          <div className="flex items-center gap-0.5">
            <ControlBtn onClick={toggleShuffle} active={shuffle} size="sm">
              <Shuffle size={16} />
            </ControlBtn>
            <ControlBtn onClick={prev}>
              <SkipBack size={20} fill="currentColor" />
            </ControlBtn>

            {/* Play/pause */}
            <button
              type="button"
              onClick={togglePlay}
              className="w-12 h-12 rounded-full bg-white/90 flex items-center justify-center text-black hover:bg-white hover:scale-105 active:scale-95 transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer mx-1.5"
            >
              {isPlaying ? (
                <Pause size={20} fill="black" strokeWidth={0} />
              ) : (
                <Play size={20} fill="black" strokeWidth={0} className="ml-0.5" />
              )}
            </button>

            <ControlBtn onClick={next}>
              <SkipForward size={20} fill="currentColor" />
            </ControlBtn>
            <ControlBtn onClick={toggleRepeat} active={repeat !== "off"} size="sm">
              {repeat === "one" ? <Repeat1 size={16} /> : <Repeat size={16} />}
            </ControlBtn>
          </div>

          {/* Time */}
          <div className="flex items-center gap-1.5">
            <span className="text-[11px] text-white/50 tabular-nums font-medium">
              {formatTime(progress)}
            </span>
            <span className="text-[11px] text-white/20">/</span>
            <span className="text-[11px] text-white/30 tabular-nums font-medium">
              {formatTime(duration)}
            </span>
          </div>
        </div>

        {/* ── Right: volume + queue ── */}
        <div className="flex items-center gap-0.5 w-[220px] justify-end">
          <ControlBtn onClick={onQueueToggle} active={queueOpen} size="sm">
            <ListMusic size={16} />
          </ControlBtn>
          <ControlBtn onClick={() => setVolume(volume > 0 ? 0 : 50)} size="sm">
            {volume === 0 ? <VolumeX size={16} /> : volume < 50 ? <Volume1 size={16} /> : <Volume2 size={16} />}
          </ControlBtn>
          <VolumeSlider volume={volume} onChange={setVolume} className="w-[100px]" />
          <span className={`text-[10px] tabular-nums w-[34px] text-right shrink-0 ${volume > 100 ? "text-amber-400/70" : "text-white/30"}`}>
            {volume}%
          </span>
        </div>
      </div>
    </div>
  );
}
