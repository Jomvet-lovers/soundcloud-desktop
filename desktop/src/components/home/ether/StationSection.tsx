import type {ReactNode} from 'react';
import React from 'react';

/** Рамка станции: частота-чип + заголовок + «почему это здесь». Секция
 *  регистрируется в тюнере через ref (data-station ставит useActiveStation). */
export const StationSection = React.memo(function StationSection({
  freq,
  title,
  why,
  refCb,
  right,
  children,
}: {
  freq: number;
  title: string;
  why: string;
  refCb: (node: HTMLElement | null) => void;
  right?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section ref={refCb} className="scroll-mt-[108px] pt-14 first:pt-9">
      <div className="mb-4 flex items-end justify-between gap-5">
        <div className="min-w-0">
          <div className="mb-1.5 flex items-center gap-2.5">
            <span
              className="rounded-md border px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums"
              style={{
                color: 'var(--color-accent)',
                borderColor: 'var(--color-accent-glow)',
                background: 'rgba(255,255,255,0.02)',
              }}
            >
              {freq.toFixed(1)}
            </span>
          </div>
          <h2 className="text-[22px] font-bold leading-tight tracking-[-0.015em] text-white/92">
            {title}
          </h2>
          <p className="mt-1 text-[13px] leading-snug text-white/50">{why}</p>
        </div>
        {right && <div className="flex-none">{right}</div>}
      </div>
      {children}
    </section>
  );
});
