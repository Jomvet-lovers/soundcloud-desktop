import { memo, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { pauseBlack14, pauseWhite14, playBlack14, playWhite14 } from '../../lib/icons';
import { type Track, usePlayerStore } from '../../stores/player';
import { type Aura, auraRgb, auraRgba, isLight } from '../../lib/aura';

interface ArtistMixButtonProps {
  tracks: Track[];
  aura: Aura;
}

function ArtistMixButtonImpl({ tracks, aura }: ArtistMixButtonProps) {
  const { t } = useTranslation();
  const playable = useMemo(
    () => tracks.filter((track) => track.enrichment?.availability !== 'wanted'),
    [tracks],
  );
  const playableUrns = useMemo(() => new Set(playable.map((t) => t.urn)), [playable]);
  const currentUrn = usePlayerStore((s) => s.currentTrack?.urn);
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const isPlayingFromMix = isPlaying && !!currentUrn && playableUrns.has(currentUrn);

  const lightAura = isLight(aura);
  const icon = isPlayingFromMix
    ? lightAura
      ? pauseBlack14
      : pauseWhite14
    : lightAura
      ? playBlack14
      : playWhite14;

  const empty = playableUrns.size === 0;

  const onClick = () => {
    if (empty) return;
    const { play, pause, resume } = usePlayerStore.getState();
    if (isPlayingFromMix) {
      pause();
      return;
    }
    if (currentUrn && playableUrns.has(currentUrn)) {
      resume();
      return;
    }
    if (playable.length === 0) return;
    play(playable[0], playable);
  };

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={empty}
      className="group relative inline-flex items-center gap-3 h-11 pl-2 pr-5 rounded-full text-[13px] font-semibold cursor-pointer transition-all duration-500 hover:scale-[1.03] active:scale-[0.97] disabled:opacity-50 disabled:cursor-not-allowed"
      style={{
        background: `linear-gradient(180deg, ${auraRgba(aura, 0.85)}, ${auraRgba(aura, 0.65)})`,
        color: lightAura ? '#000' : '#fff',
        border: `0.5px solid ${auraRgba(aura, 0.5)}`,
        boxShadow: `0 12px 32px ${auraRgba(aura, 0.45)}, inset 0 1px 0 rgba(255,255,255,0.25)`,
      }}
    >
      <span
        className="w-7 h-7 rounded-full flex items-center justify-center"
        style={{
          background: lightAura ? 'rgba(0,0,0,0.18)' : 'rgba(255,255,255,0.18)',
          border: `0.5px solid ${lightAura ? 'rgba(0,0,0,0.25)' : 'rgba(255,255,255,0.25)'}`,
        }}
      >
        {icon}
      </span>
      <span className="tracking-wide">{t('artist.playMix')}</span>
      <span
        className="absolute inset-0 rounded-full pointer-events-none opacity-0 group-hover:opacity-100 transition-opacity duration-700"
        style={{ boxShadow: `0 0 60px ${auraRgb(aura)}` }}
      />
    </button>
  );
}

export const ArtistMixButton = memo(ArtistMixButtonImpl);
