import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { type Aura, auraRgb, auraRgba, isLight } from '../../lib/aura';
import { art } from '../../lib/formatters';
import { Loader2, Shuffle, User as UserIcon } from '../../lib/icons';
import type { Track } from '../../stores/player';
import { ArtworkMosaic } from './ArtworkMosaic';
import { SoundprintBars } from './SoundprintBars';
import { useShuffleLikes } from './useShuffleLikes';
import type { Soundprint } from './useSoundprint';

interface MastheadUser {
  username: string;
  avatar_url: string;
}

function greetingKey(): string {
  const h = new Date().getHours();
  if (h < 5) return 'library.greetNight';
  if (h < 12) return 'library.greetMorning';
  if (h < 18) return 'library.greetDay';
  return 'library.greetEvening';
}

const AvatarOrb = memo(function AvatarOrb({ url, aura }: { url: string | null; aura: Aura }) {
  return (
    <div className="relative shrink-0 w-[84px] h-[84px] md:w-[100px] md:h-[100px]">
      <div
        className="relative w-full h-full rounded-full overflow-hidden transition-[border-color,box-shadow] duration-500"
        style={{ border: `0.5px solid ${auraRgba(aura, 0.4)}` }}
      >
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

/** "Sound Print" — your collection as a living portrait of your taste. */
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
  const { t } = useTranslation();
  const { shuffle, loading } = useShuffleLikes();
  const avatar = art(user.avatar_url, 't300x300');
  // Flat accent "play" surface.
  const playSurface = auraRgb(sound.aura);
  const playRing = 'inset 0 0 0 1px rgba(255,255,255,0.22)';

  return (
    <section
      className="relative overflow-hidden rounded-[2.25rem] p-6 md:p-8 transition-[box-shadow] duration-500"
      style={{
        border: '0.5px solid rgba(255,255,255,0.1)',
        boxShadow: '0 30px 80px rgba(0,0,0,0.42)',
      }}
    >
      {/* flat dark surface under the content */}
      <div
        className="absolute inset-0 rounded-[inherit] transition-[background] duration-500"
        style={{
          contain: 'strict',
          transform: 'translateZ(0)',
          background: 'rgba(20,20,24,0.9)',
        }}
      />
      <ArtworkMosaic tracks={likedTracks} />

      <div className="relative z-10 flex flex-col gap-6" style={{ isolation: 'isolate' }}>
        <div className="flex items-center gap-5">
          <AvatarOrb url={avatar} aura={sound.aura} />
          <div className="min-w-0 flex-1">
            <p className="flex items-center gap-1.5 text-[11px] text-white/40 font-medium mb-1.5">
              {t('nav.library')}
            </p>
            <h1
              className="text-[26px] md:text-[34px] font-black tracking-tight leading-[1.05] break-words"
              style={{ color: auraRgb(sound.aura) }}
            >
              {t(greetingKey(), { name: user.username })}
            </h1>
          </div>
          <button
            type="button"
            onClick={shuffle}
            disabled={loading}
            className="hidden sm:flex shrink-0 items-center gap-2.5 pl-4 pr-5 py-3 rounded-full font-bold text-[14px] cursor-pointer transition-transform duration-300 hover:scale-[1.04] active:scale-95 disabled:opacity-60"
            style={{
              color: isLight(sound.aura) ? '#0a0a0c' : '#fff',
              background: playSurface,
              boxShadow: playRing,
            }}
          >
            {loading ? <Loader2 size={18} className="animate-spin" /> : <Shuffle size={18} />}
            {t('library.playYourSound')}
          </button>
        </div>

        {sound.hasData && (
          <SoundprintBars spectrum={sound.spectrum} selected={selected} onSelect={onSelect} />
        )}

        <button
          type="button"
          onClick={shuffle}
          disabled={loading}
          className="sm:hidden flex w-fit items-center gap-2 px-4 py-3 rounded-2xl font-bold text-[13px] cursor-pointer disabled:opacity-60"
          style={{
            color: isLight(sound.aura) ? '#0a0a0c' : '#fff',
            background: playSurface,
            boxShadow: playRing,
          }}
        >
          {loading ? <Loader2 size={16} className="animate-spin" /> : <Shuffle size={16} />}
          {t('library.playYourSound')}
        </button>
      </div>
    </section>
  );
});
