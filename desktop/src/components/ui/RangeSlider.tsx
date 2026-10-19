import React from 'react';

/**
 * Flat range slider with a pointer-friendly hit area.
 *
 * The visible 4px track, accent fill and thumb are sibling divs painted behind
 * a native <input type="range"> stretched over the whole 24px band, so the
 * entire row drives the value on touch and pen while keyboard control and the
 * native event contract stay unchanged for mouse. Solid colors only: the accent
 * fill is the value hierarchy cue, deliberately not a gradient.
 */
export const RangeSlider = React.memo(function RangeSlider({
  min,
  max,
  step,
  value,
  disabled,
  onChange,
  className,
  ariaLabel,
}: {
  min: number;
  max: number;
  step?: number;
  value: number;
  disabled?: boolean;
  onChange: (v: number) => void;
  className?: string;
  ariaLabel?: string;
}) {
  const span = max - min;
  const pct = span > 0 ? (Math.min(Math.max(value, min), max) - min) / span : 0;

  return (
    <div
      className={`relative flex h-6 items-center touch-none ${
        disabled ? 'opacity-40' : ''
      } ${className ?? ''}`}
    >
      <div className="pointer-events-none absolute inset-x-0 top-1/2 h-1 -translate-y-1/2 rounded-full bg-white/[0.12]" />
      <div
        className="pointer-events-none absolute top-1/2 h-1 -translate-y-1/2 rounded-full bg-accent"
        style={{ left: 0, width: `${pct * 100}%` }}
      />
      <input
        type="range"
        aria-label={ariaLabel}
        min={min}
        max={max}
        step={step}
        value={value}
        disabled={disabled}
        onChange={(e) => onChange(Number(e.target.value))}
        className="relative z-10 h-6 w-full cursor-pointer appearance-none bg-transparent outline-none disabled:cursor-default [&::-webkit-slider-runnable-track]:h-6 [&::-webkit-slider-runnable-track]:rounded-full [&::-webkit-slider-runnable-track]:bg-transparent [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:size-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:shadow-[0_1px_3px_rgba(0,0,0,0.5)] [&:focus-visible::-webkit-slider-thumb]:shadow-[0_0_0_2px_var(--color-accent)]"
      />
    </div>
  );
});
