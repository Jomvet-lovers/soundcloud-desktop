import React from 'react';
import {useTranslation} from 'react-i18next';
import {playWhite14, RefreshCw} from '../../../lib/icons';
import {usePerfMode} from '../../../lib/perf';
import type {Track} from '../../../stores/player';
import {usePlayerStore} from '../../../stores/player';
import {HideLikedToggle} from '../../music/soundwave/hide-liked-toggle';
import {HideListenedToggle} from '../../music/soundwave/hide-listened-toggle';
import {LanguageFilter} from '../../music/soundwave/language-filter';
import {WaveTrackHeader} from '../../music/soundwave/track-header';
import {LiveWaveform} from '../../music/soundwave/waveform';

/** Индикатор «эфир подстраивается» — три живых бара, только когда играет. */
function AdaptDots() {
  const perf = usePerfMode();
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  if (!isPlaying || !perf.idleAnim) return null;
  return (
    <span className="flex items-end gap-[2.5px]" aria-hidden>
      {['-0.3s', '-0.7s', '-0.1s'].map((d) => (
        <i
          key={d}
          className="eth-anim w-[3px] origin-bottom rounded-[2px]"
          style={{
            height: 10,
            background: 'var(--color-accent)',
            animation: `eth-eq 0.9s ease-in-out ${d} infinite alternate`,
          }}
        />
      ))}
    </span>
  );
}

/** On-air дека — единственная тяжёлая blur-поверхность страницы: LIVE-шапка,
 *  играющий трек, несущая частота (waveform во всю ширину), пульт волны. */
export const OnAirDeck = React.memo(function OnAirDeck({
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
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const b = perf.blur(24);

  return (
    <section
      className="relative overflow-hidden rounded-[20px] border border-white/[0.1] shadow-[0_24px_60px_rgba(0,0,0,0.45),inset_0_1px_0_rgba(255,255,255,0.08)]"
      style={{
        background:
          b > 0
            ? 'linear-gradient(160deg, rgba(255,255,255,0.07), rgba(255,255,255,0.025) 55%)'
            : 'rgb(18,18,22)',
        backdropFilter: b > 0 ? `blur(${b}px) saturate(1.3)` : undefined,
        WebkitBackdropFilter: b > 0 ? `blur(${b}px) saturate(1.3)` : undefined,
      }}
    >
      <div
        className="pointer-events-none absolute inset-0"
        style={{
          background:
            'radial-gradient(ellipse 60% 90% at 12% 0%, var(--color-accent-glow), transparent 60%)',
          opacity: 0.55,
          contain: 'strict',
        }}
      />

      <div className="relative flex flex-col gap-4 p-5 md:p-6" style={{ isolation: 'isolate' }}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-2.5">
            <span
              className="eth-anim size-2 rounded-full"
              style={{
                background: 'var(--color-accent)',
                boxShadow: perf.glow ? '0 0 8px var(--color-accent)' : undefined,
                animation:
                  isPlaying && perf.idleAnim ? 'eth-pulse 1.6s ease-in-out infinite' : undefined,
              }}
            />
            <b className="text-[11px] font-bold uppercase tracking-[0.22em] text-white/90">
              {t('soundwave.ether.live')}
            </b>
            <span className="flex items-center gap-2 text-[11.5px] text-white/45">
              <AdaptDots />
              {t('soundwave.ether.adapts')}
            </span>
          </div>
          <span className="font-mono text-[11px] text-white/35">
            {t('soundwave.ether.queueInf')}
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

        <LiveWaveform track={track} isCurrent={isCurrent} />

        <div className="flex flex-wrap items-center gap-3 border-t border-white/[0.06] pt-4">
          <button
            type="button"
            onClick={onPlayWave}
            disabled={!canPlay}
            className="flex h-10 cursor-pointer items-center gap-2.5 rounded-[12px] bg-accent px-5 text-[13.5px] font-semibold text-accent-contrast shadow-[0_6px_24px_var(--color-accent-glow),inset_0_1px_0_rgba(255,255,255,0.25)] transition-transform hover:-translate-y-0.5 active:translate-y-0 disabled:cursor-not-allowed disabled:opacity-40"
          >
            {playWhite14}
            {t('soundwave.ether.playWave')}
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
