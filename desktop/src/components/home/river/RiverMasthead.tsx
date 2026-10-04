import {memo} from 'react';
import {useTranslation} from 'react-i18next';
import {art} from '../../../lib/formatters';
import {User as UserIcon} from '../../../lib/icons';
import {SoundprintBars} from '../../library/SoundprintBars';
import type {Soundprint} from '../../library/useSoundprint';
import {VibePortal} from '../../music/soundwave/vibe-portal';

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

/** Шапка «Эфира»: позывные станции + приветствие; ниже — спектроанализатор
 *  вкуса (клик по жанру ретинтит страницу) и vibe-портал. Без blur — лёгкая. */
export const RiverMasthead = memo(function RiverMasthead({
  user,
  sound,
  selected,
  onSelect,
}: {
  user: MastheadUser;
  sound: Soundprint;
  selected: string | null;
  onSelect: (genre: string | null) => void;
}) {
  const { t } = useTranslation();
  const avatar = art(user.avatar_url, 't300x300');

  return (
    <header className="pt-2">
      <div className="flex items-start justify-between gap-6">
        <div className="min-w-0">
          <h1 className="break-words text-[26px] font-black leading-[1.05] tracking-tight text-white/92 md:text-[32px]">
            {t(greetingKey(), { name: user.username })}
          </h1>
          <p className="mt-2 text-[13.5px] text-white/50">{t('soundwave.tagline')}</p>
        </div>

        <div className="relative size-16 flex-none overflow-hidden rounded-full ring-1 ring-white/10">
          {avatar ? (
            <img src={avatar} alt="" className="size-full object-cover" decoding="async" />
          ) : (
            <span className="flex size-full items-center justify-center bg-white/5">
              <UserIcon size={22} className="text-white/25" />
            </span>
          )}
        </div>
      </div>

      <div className="mt-6 flex flex-col items-stretch gap-4 lg:flex-row">
        {sound.hasData && (
          <div className="min-w-0 flex-1 rounded-2xl border border-white/[0.07] bg-white/[0.025] px-5 py-4">
            <SoundprintBars spectrum={sound.spectrum} selected={selected} onSelect={onSelect} />
          </div>
        )}
        <div className="flex w-full flex-none flex-col justify-end lg:w-[360px]">
          <VibePortal />
        </div>
      </div>
    </header>
  );
});
