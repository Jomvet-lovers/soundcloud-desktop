import { memo } from 'react';
import { art } from '../../lib/formatters';
import { Loader2, Pause, Play, User as UserIcon } from '../../lib/icons';
import { useIsPlayingFrom } from '../../lib/useTrackPlay';
import { type Track, usePlayerStore } from '../../stores/player';
import { SoundprintBars } from './SoundprintBars';
import { useShuffleLikes } from './useShuffleLikes';
import type { Soundprint } from './useSoundprint';

interface MastheadUser {
  username: string;
  avatar_url: string;
}

function greeting(name: string): string {
  const h = new Date().getHours();
  if (h < 5) return `Late night, ${name}`;
  if (h < 12) return `Good morning, ${name}`;
  if (h < 18) return `Good afternoon, ${name}`;
  return `Good evening, ${name}`;
}

const AvatarOrb = memo(function AvatarOrb({ url }: { url: string | null }) {
  return (
    <div className="relative shrink-0 w-[84px] h-[84px] md:w-[100px] md:h-[100px]">
      <div className="relative w-full h-full rounded-full overflow-hidden ring-1 ring-white/10">
        {url ? (
          <img src={url} alt="" className="w-full h-full object-cover" decoding="async" />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-white/5">
            <UserIcon size={30} className="text-white/25" />
          </div>
        )}
      </div>
    </div>
  );
});

/** Flat library hero — avatar, greeting, soundprint and the same circular play
 *  control as the track/playlist pages (plays your collection shuffled). */
export const SoundPrintMasthead = memo(function SoundPrintMasthead({
  user,
  likedTracks,
  sound,
  selected,
  onSelect,
}: {
  user: MastheadUser;
  likedTracks: Track[];
  sound: Soundprint;
  selected: string | null;
  onSelect: (genre: string | null) => void;
}) {
  const { shuffle, loading } = useShuffleLikes();
  const avatar = art(user.avatar_url, 't300x300');

  const likedUrns = new Set(likedTracks.map((t) => t.urn));
  const isPlayingThis = useIsPlayingFrom(likedUrns);

  const onPlay = () => {
    if (loading) return;
    const { pause, resume, currentTrack } = usePlayerStore.getState();
    if (isPlayingThis) {
      pause();
      return;
    }
    if (currentTrack && likedUrns.has(currentTrack.urn)) {
      resume();
      return;
    }
    void shuffle();
  };

  return (
    <section className="flex flex-col gap-6">
      <div className="flex items-center gap-5">
        <AvatarOrb url={avatar} />
        <div className="min-w-0 flex-1">
          <p className="text-[11px] text-white/40 font-medium mb-1.5">{'Library'}</p>
          <h1 className="text-[26px] md:text-[34px] font-black tracking-tight leading-[1.05] break-words text-white">
            {greeting(user.username)}
          </h1>
        </div>
        <button
          type="button"
          onClick={onPlay}
          disabled={loading}
          aria-label={isPlayingThis ? 'Pause' : 'Play'}
          className="w-[68px] h-[68px] shrink-0 rounded-full border border-white/[0.18] hover:border-white/[0.4] flex items-center justify-center text-white/90 hover:text-white transition-colors cursor-pointer disabled:opacity-50 disabled:cursor-default"
        >
          {loading ? (
            <Loader2 size={22} className="animate-spin" />
          ) : isPlayingThis ? (
            <span key="pause" className="animate-icon-pop flex items-center justify-center">
              <Pause size={24} fill="currentColor" strokeWidth={0} />
            </span>
          ) : (
            <span key="play" className="animate-icon-pop flex items-center justify-center">
              <Play size={24} fill="currentColor" strokeWidth={0} className="ml-1" />
            </span>
          )}
        </button>
      </div>

      {sound.hasData && (
        <SoundprintBars spectrum={sound.spectrum} selected={selected} onSelect={onSelect} />
      )}
    </section>
  );
});
