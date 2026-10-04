import React from 'react';
import { fc } from '../../lib/formatters';

interface StatOrbProps {
  value: number | null | undefined;
  label: string;
  accent: string;
}

function StatOrbImpl({ value, label, accent: _accent }: StatOrbProps) {
  return (
    <div
      className="relative flex items-baseline gap-2 rounded-xl px-3 py-1.5"
      style={{
        background: 'rgba(28,28,32,0.85)',
        border: '0.5px solid rgba(255,255,255,0.08)',
      }}
    >
      <span className="text-[15px] font-semibold tabular-nums tracking-tight text-white">
        {value != null ? fc(value) : '—'}
      </span>
      <span className="text-[10.5px] text-white/40">{label}</span>
    </div>
  );
}

export const StatOrb = React.memo(StatOrbImpl);
