import React from 'react';
import type { Aura } from '../../lib/aura';
import {usePerfMode} from '../../lib/perf';
import { PAGE_STAR_SEEDS, StarField } from './StarField';

interface AuraFieldProps {
  aura: Aura;
  isStar: boolean;
}

function AuraFieldImpl({ aura, isStar }: AuraFieldProps) {
    const perf = usePerfMode();

    if (!perf.atmosphere) {
        return (
            <div
                className="fixed inset-0 pointer-events-none overflow-hidden"
                style={{
                    contain: 'strict',
                    transform: 'translateZ(0)',
                    background: `radial-gradient(circle at 30% 20%, ${aura.orbs[0]}22 0%, transparent 60%), radial-gradient(circle at 80% 80%, ${aura.orbs[2]}1a 0%, transparent 60%)`,
                }}
            />
        );
    }

    const b0 = perf.blur(120);
    const b1 = perf.blur(140);
    const b2 = perf.blur(160);
    // Beauty keeps the original breathing scale; lighter modes use translate-only
    // drift so the huge blur kernel isn't re-rasterized every frame.
    const drift = perf.mode === 'beauty' ? 'orb-drift' : 'orb-drift-lite';

  return (
    <div
        className="fixed inset-0 pointer-events-none overflow-hidden"
      style={{ contain: 'strict', transform: 'translateZ(0)' }}
    >
      <div
        className="absolute -top-[20%] -left-[15%] w-[80vw] h-[80vw] rounded-full mix-blend-screen"
        style={{
          background: `radial-gradient(circle, ${aura.orbs[0]} 0%, transparent 65%)`,
          opacity: isStar ? 0.45 : 0.22,
            filter: `blur(${b0}px)`,
            animation: perf.idleAnim ? `${drift} 22s ease-in-out infinite` : undefined,
        }}
      />
      <div
        className="absolute top-[5%] -right-[20%] w-[70vw] h-[70vw] rounded-full mix-blend-screen"
        style={{
          background: `radial-gradient(circle, ${aura.orbs[1]} 0%, transparent 65%)`,
          opacity: isStar ? 0.4 : 0.18,
            filter: `blur(${b1}px)`,
            animation: perf.idleAnim ? `${drift} 28s ease-in-out -8s infinite` : undefined,
        }}
      />
      <div
          className="absolute -bottom-[18%] left-[16%] w-[62vw] h-[62vw] rounded-full mix-blend-screen"
        style={{
          background: `radial-gradient(circle, ${aura.orbs[2]} 0%, transparent 65%)`,
            opacity: isStar ? 0.32 : 0.14,
            filter: `blur(${b2}px)`,
            animation: perf.idleAnim ? `${drift} 34s ease-in-out -16s infinite` : undefined,
        }}
      />
      {isStar && <StarField aura={aura} seeds={PAGE_STAR_SEEDS} intensity={0.85} />}
    </div>
  );
}

export const AuraField = React.memo(AuraFieldImpl);
