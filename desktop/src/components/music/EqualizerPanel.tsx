import React, { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import {
  EQ_BAND_COUNT,
  EQ_LABELS,
  EQ_MAX_GAIN,
  EQ_MIN_GAIN,
  EQ_PRESETS,
} from '../../lib/equalizer';
import { AudioLines, Power, RotateCcw, X } from '../../lib/icons';
import { useSettingsStore } from '../../stores/settings';
import { Modal, ModalClose, ModalContent, ModalTrigger } from '../ui/Modal';

/* ── Single Band Slider ─────────────────────────────────────── */

const BandSlider = React.memo(function BandSlider({
  index,
  gain,
  label,
  onChange,
}: {
  index: number;
  gain: number;
  label: string;
  onChange: (index: number, gain: number) => void;
}) {
  const trackRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const calcGain = useCallback((clientY: number) => {
    const track = trackRef.current;
    if (!track) return 0;
    const rect = track.getBoundingClientRect();
    const pct = 1 - Math.max(0, Math.min(1, (clientY - rect.top) / rect.height));
    return Math.round((pct * (EQ_MAX_GAIN - EQ_MIN_GAIN) + EQ_MIN_GAIN) * 2) / 2;
  }, []);

  const handlePointerDown = useCallback(
    (e: React.PointerEvent) => {
      e.preventDefault();
      dragging.current = true;
      (e.target as HTMLElement).setPointerCapture(e.pointerId);
      onChange(index, calcGain(e.clientY));
    },
    [index, onChange, calcGain],
  );

  const handlePointerMove = useCallback(
    (e: React.PointerEvent) => {
      if (!dragging.current) return;
      onChange(index, calcGain(e.clientY));
    },
    [index, onChange, calcGain],
  );

  const handlePointerUp = useCallback(() => {
    dragging.current = false;
  }, []);

  // Wheel: one notch per event on the same 0.5 dB grid as dragging, mapped so
  // scrolling up raises the band. Non-passive so the modal does not scroll.
  useEffect(() => {
    const el = trackRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY === 0) return;
      e.preventDefault();
      const next = gain + (e.deltaY < 0 ? 0.5 : -0.5);
      const clamped = Math.max(EQ_MIN_GAIN, Math.min(EQ_MAX_GAIN, next));
      onChange(index, Math.round(clamped * 2) / 2);
    };
    el.addEventListener('wheel', onWheel, { passive: false });
    return () => el.removeEventListener('wheel', onWheel);
  }, [gain, index, onChange]);

  // Normalized 0-1 position
  const pct = (gain - EQ_MIN_GAIN) / (EQ_MAX_GAIN - EQ_MIN_GAIN);
  const isPositive = gain > 0;
  const isNegative = gain < 0;

  return (
    <div className="flex flex-col items-center gap-2 select-none">
      {/* Gain value */}
      <span
        className={`text-[10px] tabular-nums font-semibold h-4 ${
          isPositive ? 'text-emerald-400' : isNegative ? 'text-blue-400' : 'text-white/35'
        }`}
      >
        {gain > 0 ? '+' : ''}
        {gain.toFixed(1)}
      </span>

      {/* Track */}
      <div
        ref={trackRef}
        className="relative w-7 h-[140px] flex items-center justify-center cursor-pointer touch-none"
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
      >
        {/* Rail */}
        <div className="absolute w-[3px] h-full rounded-full bg-white/[0.1]" />

        {/* Center line */}
        <div className="absolute w-2 h-px bg-white/[0.14] left-1/2 top-1/2 -translate-x-1/2" />

        {/* Fill */}
        <div
          className="absolute w-[3px] rounded-full left-1/2 -translate-x-1/2"
          style={{
            bottom: gain >= 0 ? '50%' : `${pct * 100}%`,
            top: gain >= 0 ? `${(1 - pct) * 100}%` : '50%',
            background: isPositive
              ? 'rgba(52,211,153,0.55)'
              : isNegative
                ? 'rgba(96,165,250,0.55)'
                : 'transparent',
          }}
        />

        {/* Thumb */}
        <div
          className="absolute left-1/2 -translate-x-1/2 w-4 h-4 rounded-full"
          style={{
            bottom: `calc(${pct * 100}% - 8px)`,
            background: isPositive
              ? 'rgb(52,211,153)'
              : isNegative
                ? 'rgb(96,165,250)'
                : 'rgba(255,255,255,0.75)',
          }}
        />
      </div>

      {/* Frequency label */}
      <span className="text-[9px] text-white/40 font-medium">{label}</span>
    </div>
  );
});

/* ── Preset Button ──────────────────────────────────────────── */

const PresetBtn = React.memo(function PresetBtn({
  id,
  label,
  active,
  onClick,
}: {
  id: string;
  label: string;
  active: boolean;
  onClick: (id: string) => void;
}) {
  return (
    <button
      type="button"
      // No document view transition here: the panel is a portalled dialog and a
      // page-wide snapshot left its surface unpainted. The pill still cross-fades
      // through `transition-colors`.
      onClick={() => onClick(id)}
      className={`group relative px-3 py-1.5 rounded-lg text-[11px] font-semibold transition-colors cursor-pointer ${
        active ? 'text-black' : 'text-white/50 hover:text-white/80'
      }`}
    >
      <span
        aria-hidden
        className={`pointer-events-none absolute inset-0 rounded-lg border transition-colors ${
          active
            ? 'bg-white border-white'
            : 'bg-white/[0.04] border-white/[0.08] group-hover:bg-white/[0.08]'
        }`}
      />
      <span className="relative">{label}</span>
    </button>
  );
});

/* ── Main Panel ─────────────────────────────────────────────── */

export const EqualizerPanel = React.memo(function EqualizerPanel({
  children,
}: {
  children: React.ReactNode;
}) {
  const eqEnabled = useSettingsStore((s) => s.eqEnabled);
  const eqGains = useSettingsStore((s) => s.eqGains);
  const eqPreset = useSettingsStore((s) => s.eqPreset);
  const eqProfiles = useSettingsStore((s) => s.eqProfiles);
  const setEqEnabled = useSettingsStore((s) => s.setEqEnabled);
  const selectEqProfile = useSettingsStore((s) => s.selectEqProfile);
  const createEqProfile = useSettingsStore((s) => s.createEqProfile);
  const deleteEqProfile = useSettingsStore((s) => s.deleteEqProfile);
  const resetEqProfile = useSettingsStore((s) => s.resetEqProfile);
  const setEqBand = useSettingsStore((s) => s.setEqBand);

  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState('');
  const nameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (creating) nameRef.current?.focus();
  }, [creating]);

  const handleBandChange = useCallback(
    (index: number, gain: number) => {
      setEqBand(index, gain);
    },
    [setEqBand],
  );

  const handleProfile = useCallback(
    (id: string) => {
      selectEqProfile(id);
    },
    [selectEqProfile],
  );

  const submitProfile = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      const label = newName.trim();
      if (!label) return;
      if (eqProfiles.some((p) => p.label.toLowerCase() === label.toLowerCase())) {
        toast.error('A profile with that name already exists');
        return;
      }
      createEqProfile(label);
      setNewName('');
      setCreating(false);
    },
    [createEqProfile, eqProfiles, newName],
  );

  const activeIsUser = eqProfiles.some((p) => p.id === eqPreset);

  // Factory presets first, then the profiles the user has saved.
  const profileEntries = [
    ...Object.entries(EQ_PRESETS).map(([id, preset]) => ({ id, label: preset.label })),
    ...eqProfiles.map((p) => ({ id: p.id, label: p.label })),
  ];

  return (
    <Modal>
      <ModalTrigger asChild>{children}</ModalTrigger>
      <ModalContent size="md" showClose={false} zClass="z-[90]">
        {/* Header */}
        <div className="flex items-center justify-between px-6 pt-5 pb-3">
          <div className="flex items-center gap-3">
            <div className="w-9 h-9 rounded-xl bg-white/[0.06] flex items-center justify-center">
              <AudioLines size={18} className="text-white/60" />
            </div>
            <h2 className="text-[17px] font-bold text-white/90 tracking-tight">{'Equalizer'}</h2>
          </div>
          <div className="flex items-center gap-2">
            {/* Power toggle */}
            <button
              type="button"
              onClick={() => setEqEnabled(!eqEnabled)}
              className={`w-9 h-9 rounded-xl flex items-center justify-center transition-colors cursor-pointer border ${
                eqEnabled
                  ? 'bg-emerald-500/15 text-emerald-400 border-emerald-500/30'
                  : 'bg-white/[0.04] text-white/25 border-white/[0.06] hover:text-white/50'
              }`}
            >
              <Power size={15} />
            </button>
            {/* Reset */}
            <button
              type="button"
              onClick={resetEqProfile}
              className="w-9 h-9 rounded-xl bg-white/[0.04] border border-white/[0.06] flex items-center justify-center text-white/25 hover:text-white/50 transition-all cursor-pointer"
            >
              <RotateCcw size={14} />
            </button>
            {/* Close */}
            <ModalClose className="w-9 h-9 rounded-xl bg-white/[0.04] border border-white/[0.06] flex items-center justify-center text-white/25 hover:text-white/50 transition-all cursor-pointer">
              <X size={15} />
            </ModalClose>
          </div>
        </div>

        {/* dB scale + Sliders */}
        <div
          className={`px-6 pb-4 transition-opacity duration-300 ${eqEnabled ? '' : 'opacity-30 pointer-events-none'}`}
        >
          <div className="flex items-end gap-0">
            {/* dB labels */}
            <div className="flex flex-col justify-between h-[140px] mr-2 -mt-6">
              <span className="text-[9px] text-white/35 tabular-nums">+12</span>
              <span className="text-[9px] text-white/35 tabular-nums">0</span>
              <span className="text-[9px] text-white/35 tabular-nums">-12</span>
            </div>
            {/* Band sliders */}
            <div className="flex-1 flex justify-between">
              {Array.from({ length: EQ_BAND_COUNT }, (_, i) => (
                <BandSlider
                  key={i}
                  index={i}
                  gain={eqGains[i] ?? 0}
                  label={EQ_LABELS[i]}
                  onChange={handleBandChange}
                />
              ))}
            </div>
          </div>
        </div>

        {/* Profiles */}
        <div
          // Stays clickable while the EQ is off: applying a profile also turns
          // it on, so the dimmed state is a hint rather than a lock.
          className={`px-6 pb-5 transition-opacity duration-300 ${eqEnabled ? '' : 'opacity-60'}`}
        >
          <p className="text-[11px] text-white/40 font-medium mb-2.5">{'Profiles'}</p>
          <div className="flex flex-wrap items-center gap-1.5">
            {profileEntries.map((entry) => (
              <PresetBtn
                key={entry.id}
                id={entry.id}
                label={entry.label}
                active={eqPreset === entry.id}
                onClick={handleProfile}
              />
            ))}
            {creating ? (
              <form onSubmit={submitProfile} className="flex items-center gap-1">
                <input
                  ref={nameRef}
                  value={newName}
                  onChange={(e) => setNewName(e.target.value)}
                  maxLength={24}
                  placeholder={'Profile name'}
                  aria-label={'New profile name'}
                  className="h-7 w-36 rounded-lg border border-white/[0.12] bg-white/[0.06] px-2 text-[11px] text-white/90 outline-none placeholder:text-white/25 focus:border-white/25"
                />
                <button
                  type="submit"
                  disabled={!newName.trim()}
                  className="h-7 rounded-lg bg-white px-2.5 text-[11px] font-semibold text-black transition-opacity disabled:opacity-40"
                >
                  {'Add'}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    setCreating(false);
                    setNewName('');
                  }}
                  className="h-7 px-1.5 text-[11px] text-white/45 hover:text-white/80"
                >
                  {'Cancel'}
                </button>
              </form>
            ) : (
              <button
                type="button"
                onClick={() => setCreating(true)}
                className="px-3 py-1.5 rounded-lg text-[11px] font-semibold text-white/45 border border-dashed border-white/[0.15] hover:text-white/80 hover:border-white/30 transition-colors cursor-pointer"
              >
                {'+ New profile'}
              </button>
            )}
            {activeIsUser && !creating && (
              <button
                type="button"
                onClick={() => deleteEqProfile(eqPreset)}
                className="px-2 py-1.5 text-[10px] text-white/30 hover:text-red-400 transition-colors cursor-pointer"
              >
                {'Delete profile'}
              </button>
            )}
          </div>
        </div>
      </ModalContent>
    </Modal>
  );
});
