import React from 'react';
import {useTranslation} from 'react-i18next';
import {playWhite14, RefreshCw} from '../../../lib/icons';
import {usePerfMode} from '../../../lib/perf';
import type {Track} from '../../../stores/player';
import {HideLikedToggle} from '../../music/soundwave/hide-liked-toggle';
import {HideListenedToggle} from '../../music/soundwave/hide-listened-toggle';
import {LanguageFilter} from '../../music/soundwave/language-filter';
import {WaveTrackHeader} from '../../music/soundwave/track-header';
import {LiveWaveform} from '../../music/soundwave/waveform';

/** On-air дека — LIVE-шапка, играющий трек, несущая частота (waveform во всю
 *  ширину), пульт волны. */
export const EstuaryDeck = React.memo(function EstuaryDeck({
  track,
  queue,
  isCurrent,
  hideListened,
  onHideListened,
  hideLiked,
  onHideLiked,
  languages,
  onLanguages,
  spinning,
  onRefresh,
  onPlayWave,
  canPlay,
}: {
  track: Track | null;
  queue: Track[];
  isCurrent: boolean;
  hideListened: boolean;
  onHideListened: (v: boolean) => void;
  hideLiked: boolean;
  onHideLiked: (v: boolean) => void;
  languages: string[];
  onLanguages: (langs: string[]) => void;
  spinning: boolean;
  onRefresh: () => void;
  onPlayWave: () => void;
  canPlay: boolean;
}) {
  const { t } = useTranslation();
  const perf = usePerfMode();

  return (
    <section
      className="relative overflow-hidden rounded-[20px] border border-white/[0.1]"
      style={{ background: 'rgb(18,18,22)' }}
    >
      <div className="relative flex flex-col gap-4 p-5 md:p-6" style={{ isolation: 'isolate' }}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2 text-[11.5px] text-white/45">
            {t('soundwave.river.adapts')}
          </div>
          <span className="font-mono text-[11px] text-white/35">
            {t('soundwave.river.queueInf')}
          </span>
        </div>

        {track ? (
          <WaveTrackHeader track={track} queue={queue} isCurrent={isCurrent} />
        ) : (
          <div className="flex items-center gap-3">
            <div className="size-14 shrink-0 rounded-xl bg-white/[0.04] ring-1 ring-white/[0.06]" />
            <div className="min-w-0 flex-1">
              <p className="text-[15px] font-semibold leading-tight text-white/90">
                {t('soundwave.idleTitle')}
              </p>
              <p className="mt-0.5 truncate text-[12px] text-white/45">{t('soundwave.idleSub')}</p>
            </div>
          </div>
        )}

        <div
          style={
            perf.mode === 'beauty'
              ? {
                  WebkitBoxReflect:
                    'below 2px linear-gradient(transparent 62%, rgba(255,255,255,0.13))',
                }
              : undefined
          }
        >
          <LiveWaveform track={track} isCurrent={isCurrent} />
        </div>

        <div className="flex flex-wrap items-center gap-3 border-t border-white/[0.06] pt-4">
          <button
            type="button"
            onClick={onPlayWave}
            disabled={!canPlay}
            className="flex h-10 cursor-pointer items-center gap-2.5 rounded-[12px] bg-accent px-5 text-[13.5px] font-semibold text-accent-contrast shadow-[0_6px_24px_var(--color-accent-glow),inset_0_1px_0_rgba(255,255,255,0.25)] transition-transform hover:-translate-y-0.5 active:translate-y-0 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {playWhite14}
            {t('soundwave.river.playWave')}
          </button>
          <span className="h-6 w-px bg-white/[0.07]" />
          <HideListenedToggle value={hideListened} onChange={onHideListened} />
          <HideLikedToggle value={hideLiked} onChange={onHideLiked} />
          <LanguageFilter selected={languages} onChange={onLanguages} />
          <button
            type="button"
            onClick={onRefresh}
            disabled={spinning}
            title={t('soundwave.refresh')}
            className="flex size-8 cursor-pointer items-center justify-center rounded-full border border-white/[0.08] bg-white/[0.06] text-white/70 transition-colors hover:border-white/[0.14] hover:bg-white/[0.1] hover:text-white/95 disabled:cursor-not-allowed disabled:opacity-40"
          >
            <RefreshCw size={13} className={spinning ? 'animate-spin' : ''} />
          </button>
        </div>
      </div>
    </section>
  );
});
