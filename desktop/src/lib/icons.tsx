/**
 * Pre-rendered icon constants — referentially stable across renders.
 * Avoids creating new JSX on every render cycle in hot-path list components.
 */
import {
  Headphones,
  Heart,
  ListMusic,
  Music,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from 'lucide-react';

// ── Play / Pause (filled, black) ─────────────────────────────
export const playBlack11 = <Play size={11} fill="black" strokeWidth={0} className="ml-px" />;
export const pauseBlack11 = <Pause size={11} fill="black" strokeWidth={0} />;

export const playBlack12 = <Play size={12} fill="black" strokeWidth={0} className="ml-px" />;
export const pauseBlack12 = <Pause size={12} fill="black" strokeWidth={0} />;

export const playBlack14 = <Play size={14} fill="black" strokeWidth={0} className="ml-px" />;
export const pauseBlack14 = <Pause size={14} fill="black" strokeWidth={0} />;

export const playBlack18 = <Play size={18} fill="black" strokeWidth={0} className="ml-0.5" />;
export const pauseBlack18 = <Pause size={18} fill="black" strokeWidth={0} />;

export const playBlack20 = <Play size={20} fill="black" strokeWidth={0} className="ml-0.5" />;
export const pauseBlack20 = <Pause size={20} fill="black" strokeWidth={0} />;

export const playBlack22 = <Play size={22} fill="black" strokeWidth={0} className="ml-0.5" />;
export const pauseBlack22 = <Pause size={22} fill="black" strokeWidth={0} />;

// ── Play / Pause (filled, white) ─────────────────────────────
export const playWhite12 = <Play size={12} fill="white" strokeWidth={0} className="ml-px" />;
export const pauseWhite12 = <Pause size={12} fill="white" strokeWidth={0} />;

export const playWhite14 = <Play size={14} fill="white" strokeWidth={0} className="ml-0.5" />;
export const pauseWhite14 = <Pause size={14} fill="white" strokeWidth={0} />;

export const playWhite16 = <Play size={16} fill="white" strokeWidth={0} className="ml-0.5" />;

// ── Play / Pause (filled, currentColor) ──────────────────────
export const playCurrent16 = <Play size={16} fill="currentColor" strokeWidth={0} />;
export const pauseCurrent16 = <Pause size={16} fill="currentColor" strokeWidth={0} />;

// ── Play / Pause (outline / misc) ────────────────────────────
export const playIcon32 = <Play size={32} />;
export const playBlack20ml1 = <Play size={20} fill="black" className="ml-1" />;
export const pauseTextWhite12 = <Pause size={12} className="text-white" />;

// ── Transport controls ───────────────────────────────────────
export const skipBack20 = <SkipBack size={20} fill="currentColor" />;
export const skipForward20 = <SkipForward size={20} fill="currentColor" />;
export const shuffleIcon16 = <Shuffle size={16} />;
export const repeatIcon16 = <Repeat size={16} />;
export const repeat1Icon16 = <Repeat1 size={16} />;

// ── Volume ───────────────────────────────────────────────────
export const volumeXIcon16 = <VolumeX size={16} />;
export const volume1Icon16 = <Volume1 size={16} />;
export const volume2Icon16 = <Volume2 size={16} />;

// ── Info icons (small, for stats) ────────────────────────────
export const headphones9 = <Headphones size={9} />;
export const headphones11 = <Headphones size={11} className="text-white/20" />;
export const heart9 = <Heart size={9} />;
export const heart11 = <Heart size={11} className="text-white/20" />;
export const listMusic8 = <ListMusic size={8} />;
export const listMusic9 = <ListMusic size={9} />;
export const listMusic16 = <ListMusic size={16} />;
export const musicIcon12 = <Music size={12} className="text-white/15" />;
export const musicIcon14 = <Music size={14} className="text-white/15" />;
export const musicIcon22 = <Music size={22} className="text-white/15" />;
export const musicIcon20 = <Music size={16} className="text-white/20" />;
