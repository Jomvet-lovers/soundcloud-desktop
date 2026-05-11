import React from 'react';
import type { Aura } from '../../lib/aura';
import { PAGE_STAR_SEEDS, StarField } from './StarField';

interface AuraFieldProps {
  aura: Aura;
  isStar: boolean;
}

function AuraFieldImpl({ aura, isStar }: AuraFieldProps) {
  return (
    <div
      className="absolute inset-0 pointer-events-none overflow-hidden"
      style={{ contain: 'strict', transform: 'translateZ(0)' }}
    >
      <div
        className="absolute -top-[20%] -left-[15%] w-[80vw] h-[80vw] rounded-full mix-blend-screen"
        style={{
          background: `radial-gradient(circle, ${aura.orbs[0]} 0%, transparent 65%)`,
          opacity: isStar ? 0.45 : 0.22,
          filter: 'blur(120px)',
          animation: 'orb-drift 22s ease-in-out infinite',
        }}
      />
      <div
        className="absolute top-[5%] -right-[20%] w-[70vw] h-[70vw] rounded-full mix-blend-screen"
        style={{
          background: `radial-gradient(circle, ${aura.orbs[1]} 0%, transparent 65%)`,
          opacity: isStar ? 0.4 : 0.18,
          filter: 'blur(140px)',
          animation: 'orb-drift 28s ease-in-out -8s infinite',
        }}
      />
      <div
        className="absolute top-[40%] left-[20%] w-[55vw] h-[55vw] rounded-full mix-blend-screen"
        style={{
          background: `radial-gradient(circle, ${aura.orbs[2]} 0%, transparent 65%)`,
          opacity: isStar ? 0.3 : 0.12,
          filter: 'blur(160px)',
          animation: 'orb-drift 34s ease-in-out -16s infinite',
        }}
      />
      {isStar && <StarField aura={aura} seeds={PAGE_STAR_SEEDS} intensity={0.85} />}
    </div>
  );
}

export const AuraField = React.memo(AuraFieldImpl);
