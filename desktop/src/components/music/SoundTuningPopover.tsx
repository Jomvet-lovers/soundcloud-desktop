import * as Popover from '@radix-ui/react-popover';
import React from 'react';
import { audioLines16 } from '../../lib/icons';
import { withViewTransition } from '../../lib/view-transition';
import {
  getEffectivePitchSemitones,
  PITCH_SEMITONES_MAX,
  PITCH_SEMITONES_MIN,
  PITCH_SEMITONES_STEP,
  PLAYBACK_RATE_MAX,
  PLAYBACK_RATE_MIN,
  PLAYBACK_RATE_STEP,
  usePlayerStore,
} from '../../stores/player';

const formatPlaybackRate = (rate: number) =>
  `${rate
    .toFixed(2)
    .replace(/\.00$/, '')
    .replace(/(\.\d)0$/, '$1')}x`;

const formatPitchSemitones = (semi: number) => {
  if (Math.abs(semi) < 0.001) return '0';
  return `${semi > 0 ? '+' : ''}${semi.toFixed(1).replace(/\.0$/, '')}`;
};

const rangeClass = 'h-1 w-full cursor-pointer disabled:cursor-default disabled:opacity-40';

/** Flat popover with playback speed and pitch controls, anchored in the player bar. */
export const SoundTuningPopover = React.memo(function SoundTuningPopover() {
  const playbackRate = usePlayerStore((s) => s.playbackRate);
  const setPlaybackRate = usePlayerStore((s) => s.setPlaybackRate);
  const resetPlaybackRate = usePlayerStore((s) => s.resetPlaybackRate);
  const pitchSemitones = usePlayerStore((s) => s.pitchSemitones);
  const pitchMode = usePlayerStore((s) => s.pitchControlMode);
  const setPitchSemitones = usePlayerStore((s) => s.setPitchSemitones);
  const resetPitchSemitones = usePlayerStore((s) => s.resetPitchSemitones);
  const setPitchControlMode = usePlayerStore((s) => s.setPitchControlMode);

  const isManual = pitchMode === 'manual';
  const effectivePitch = getEffectivePitchSemitones(playbackRate, pitchMode, pitchSemitones);
  const rateIsDefault = Math.abs(playbackRate - 1) < 0.001;
  const pitchIsDefault = !isManual || Math.abs(pitchSemitones) < 0.001;
  const active = !rateIsDefault || !pitchIsDefault;

  return (
    <Popover.Root>
      <Popover.Trigger asChild>
        <button
          type="button"
          className={`flex size-8 items-center justify-center rounded-full transition-colors hover:bg-white/[0.06] ${
            active ? 'text-accent' : 'text-white/55 hover:text-white/90'
          }`}
          title={'Sound tuning'}
        >
          {audioLines16}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          side="top"
          align="end"
          sideOffset={10}
          collisionPadding={12}
          className="z-[200] w-[280px] rounded-xl border border-white/[0.1] bg-[#141417] p-4 outline-none"
        >
          <p className="mb-3 text-[11px] font-medium text-white/45">{'Sound tuning'}</p>

          {/* Playback speed */}
          <div className="mb-4">
            <div className="mb-2 flex items-center justify-between">
              <span className="text-[11px] text-white/55">{'Speed'}</span>
              <button
                type="button"
                disabled={rateIsDefault}
                onClick={() => resetPlaybackRate()}
                title={'Reset to 1.00x'}
                className={`text-[11px] tabular-nums transition-colors ${
                  rateIsDefault
                    ? 'text-white/40'
                    : 'text-accent hover:text-accent/80 cursor-pointer'
                }`}
              >
                {formatPlaybackRate(playbackRate)}
              </button>
            </div>
            <input
              type="range"
              min={PLAYBACK_RATE_MIN}
              max={PLAYBACK_RATE_MAX}
              step={PLAYBACK_RATE_STEP}
              value={playbackRate}
              onChange={(e) => setPlaybackRate(Number(e.target.value))}
              className={rangeClass}
              style={{ accentColor: 'var(--color-accent)' }}
              aria-label={'Speed'}
            />
          </div>

          {/* Pitch */}
          <div>
            <div className="mb-2 flex items-center justify-between gap-2">
              <div className="flex items-center gap-2">
                <span className="text-[11px] text-white/55">{'Pitch'}</span>
                <div className="flex overflow-hidden rounded-md border border-white/[0.1]">
                  <button
                    type="button"
                    onClick={() => withViewTransition(() => setPitchControlMode('auto'))}
                    title={'Pitch follows speed'}
                    className={`relative h-5 px-1.5 text-[9px] transition-colors ${
                      !isManual ? 'text-white' : 'text-white/45 hover:text-white/75'
                    }`}
                  >
                    <span
                      aria-hidden
                      className={`pointer-events-none absolute inset-0 ${
                        !isManual ? 'vt-pitch-pill bg-white/[0.14]' : ''
                      }`}
                    />
                    <span className="relative">{'Auto'}</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => withViewTransition(() => setPitchControlMode('manual'))}
                    title={'Manual pitch'}
                    className={`relative h-5 border-l border-white/[0.1] px-1.5 text-[9px] transition-colors ${
                      isManual ? 'text-white' : 'text-white/45 hover:text-white/75'
                    }`}
                  >
                    <span
                      aria-hidden
                      className={`pointer-events-none absolute inset-0 ${
                        isManual ? 'vt-pitch-pill bg-white/[0.14]' : ''
                      }`}
                    />
                    <span className="relative">{'Manual'}</span>
                  </button>
                </div>
              </div>
              <button
                type="button"
                disabled={pitchIsDefault}
                onClick={() => resetPitchSemitones()}
                title={'Reset pitch'}
                className={`text-[11px] tabular-nums transition-colors ${
                  pitchIsDefault
                    ? 'text-white/40'
                    : 'text-accent hover:text-accent/80 cursor-pointer'
                }`}
              >
                {formatPitchSemitones(effectivePitch)}
              </button>
            </div>
            <input
              type="range"
              min={PITCH_SEMITONES_MIN}
              max={PITCH_SEMITONES_MAX}
              step={PITCH_SEMITONES_STEP}
              value={effectivePitch}
              disabled={!isManual}
              onChange={(e) => setPitchSemitones(Number(e.target.value))}
              className={rangeClass}
              style={{ accentColor: 'var(--color-accent)' }}
              aria-label={'Pitch'}
            />
          </div>
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
});
