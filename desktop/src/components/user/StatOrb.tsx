import React from 'react';
import { fc } from '../../lib/formatters';

interface StatOrbProps {
  value: number | null | undefined;
  label: string;
  accent: string;
}

function StatOrbImpl({ value, label, accent }: StatOrbProps) {
  return (
    <div
      className="relative px-5 py-3 rounded-2xl flex items-baseline gap-2.5 transition-all duration-500 hover:scale-[1.04]"
      style={{
        background: 'rgba(255,255,255,0.04)',
        border: '0.5px solid rgba(255,255,255,0.08)',
        backdropFilter: 'blur(24px)',
        WebkitBackdropFilter: 'blur(24px)',
        boxShadow: `inset 0 0.5px 0 rgba(255,255,255,0.08), 0 8px 24px ${accent}`,
      }}
    >
      <span className="text-[20px] font-black tabular-nums tracking-tight text-white">
        {value != null ? fc(value) : '—'}
      </span>
      <span className="text-[10px] font-bold uppercase tracking-[0.22em] text-white/40">
        {label}
      </span>
    </div>
  );
}

export const StatOrb = React.memo(StatOrbImpl);
