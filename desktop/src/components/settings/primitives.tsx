import type { ReactNode } from 'react';

/** Shared settings primitives — one consistent visual language across every card. */

export function Toggle({
  checked,
  onChange,
  disabled = false,
}: {
  checked: boolean;
  onChange: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={onChange}
      className={`relative w-11 h-6 rounded-full transition-all duration-200 shrink-0 ${
        disabled ? 'opacity-40 cursor-not-allowed' : 'cursor-pointer'
      } ${checked ? 'bg-accent' : 'bg-white/10'}`}
    >
      <span
        className={`absolute top-0.5 w-5 h-5 rounded-full shadow-md transition-all duration-200 ${
          checked ? 'left-[22px] bg-accent-contrast' : 'left-0.5 bg-white'
        }`}
      />
    </button>
  );
}

export function Row({
  title,
  desc,
  children,
}: {
  title: ReactNode;
  desc?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-3 first:pt-0 last:pb-0">
      <div className="min-w-0">
        <div className="text-[13.5px] text-white/80 font-medium flex items-center gap-2">
          {title}
        </div>
        {desc && <p className="text-[11.5px] text-white/35 mt-0.5 leading-snug">{desc}</p>}
      </div>
      {children && <div className="shrink-0 flex items-center gap-2">{children}</div>}
    </div>
  );
}

export function Divider() {
  return <div className="border-t border-white/[0.05]" />;
}

export function Card({
  title,
  desc,
  icon,
  action,
  children,
}: {
  title: string;
  desc?: string;
  icon?: ReactNode;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section
      className="group relative rounded-3xl p-6 overflow-hidden transition-[box-shadow,border-color] duration-500 hover:border-white/[0.14]"
      style={{
        border: '0.5px solid rgba(255,255,255,0.1)',
        background: 'rgba(20,20,24,0.85)',
        boxShadow: '0 18px 50px rgba(0,0,0,0.4)',
      }}
    >
      {/* top specular hairline */}
      <span
        aria-hidden
        className="absolute inset-x-6 top-0 h-px"
        style={{
          background: 'rgba(255,255,255,0.06)',
        }}
      />
      <div className="relative flex items-start justify-between gap-4 mb-5">
        <div className="flex items-center gap-3 min-w-0">
          {icon && (
            <div
              className="w-9 h-9 rounded-xl flex items-center justify-center shrink-0 text-[var(--color-accent)]"
              style={{
                background: 'rgba(255,255,255,0.06)',
                border: '0.5px solid rgba(255,255,255,0.1)',
              }}
            >
              {icon}
            </div>
          )}
          <div className="min-w-0">
            <h3 className="text-[15px] font-bold text-white/85 tracking-tight">{title}</h3>
            {desc && <p className="text-[11.5px] text-white/35 mt-0.5 leading-snug">{desc}</p>}
          </div>
        </div>
        {action && <div className="shrink-0">{action}</div>}
      </div>
      {children}
    </section>
  );
}

export function Segmented<T extends string>({
  value,
  options,
  onChange,
  columns,
}: {
  value: T;
  options: ReadonlyArray<{ id: T; label: string }>;
  onChange: (v: T) => void;
  columns?: number;
}) {
  return (
    <div
      className="grid gap-2"
      style={{ gridTemplateColumns: `repeat(${columns ?? options.length}, minmax(0,1fr))` }}
    >
      {options.map((o) => {
        const active = o.id === value;
        return (
          <button
            key={o.id}
            type="button"
            onClick={() => onChange(o.id)}
            className={`rounded-xl border px-3 py-2.5 text-[12.5px] font-semibold transition-all duration-200 cursor-pointer ${
              active
                ? 'text-white'
                : 'text-white/45 hover:text-white/70 hover:bg-white/[0.05] border-white/[0.05] bg-white/[0.02]'
            }`}
            style={
              active
                ? {
                    background: 'rgba(255,255,255,0.08)',
                    borderColor: 'var(--color-accent)',
                  }
                : undefined
            }
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}

export function RangeSlider({
  value,
  min,
  max,
  step,
  onChange,
}: {
  value: number;
  min: number;
  max: number;
  step: number;
  onChange: (v: number) => void;
}) {
  return (
    <input
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className="w-full accent-[var(--color-accent)] h-1 bg-white/10 rounded-full appearance-none cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-white [&::-webkit-slider-thumb]:shadow-lg"
    />
  );
}
