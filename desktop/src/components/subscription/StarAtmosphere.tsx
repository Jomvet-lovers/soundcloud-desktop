import { memo } from 'react';
import { usePerfMode } from '../../lib/perf';

/* Fixed accent-anchored halo field behind the STAR PASS flow. Warm metal glow
 * top-right + a low bloom bottom so the viewport edges aren't dark (immersive
 * rules: fixed inset-0 + contain:strict + translateZ(0); content owns z-10). */
export const StarAtmosphere = memo(function StarAtmosphere() {
  const perf = usePerfMode();

  // Light: a single flat radial tint — no blur, no drift, no blend.
  if (!perf.atmosphere) {
    return (
      <div
        className="fixed inset-0 -z-0 overflow-hidden pointer-events-none"
        style={{ contain: 'strict', transform: 'translateZ(0)' }}
      >
        <div
          className="absolute inset-0"
          style={{
            background:
              'radial-gradient(70vw 60vh at 80% -6%, color-mix(in srgb, var(--color-accent) 12%, transparent), transparent 60%),' +
              ' radial-gradient(90vw 70vh at 50% 116%, color-mix(in srgb, var(--color-accent) 7%, transparent), transparent 55%)',
          }}
        />
      </div>
    );
  }

  const idle = perf.idleAnim;
  const blur = perf.blur(120);

  return (
    <div
      className="fixed inset-0 -z-0 overflow-hidden pointer-events-none"
      style={{ contain: 'strict', transform: 'translateZ(0)' }}
    >
      {/* base radial wash */}
      <div
        className="absolute inset-0"
        style={{
          background:
            'radial-gradient(70vw 60vh at 80% -8%, color-mix(in srgb, var(--color-accent) 18%, transparent), transparent 60%),' +
            ' radial-gradient(60vw 50vh at 8% 12%, color-mix(in srgb, var(--color-accent) 10%, transparent), transparent 60%),' +
            ' radial-gradient(90vw 70vh at 50% 120%, color-mix(in srgb, var(--color-accent) 9%, transparent), transparent 55%)',
        }}
      />
      {/* drifting accent halo (top-right) */}
      <div
        className="absolute -right-[18%] -top-[20%] h-[70vw] w-[70vw] rounded-full mix-blend-screen"
        style={{
          background: 'radial-gradient(circle, var(--color-accent) 0%, transparent 62%)',
          opacity: 0.28,
          filter: `blur(${blur}px)`,
          animation: idle ? 'star-halo 26s ease-in-out infinite' : undefined,
        }}
      />
      {/* low bloom (bottom) so the viewport floor glows under the floating player */}
      <div
        className="absolute -bottom-[24%] left-[14%] h-[60vw] w-[60vw] rounded-full mix-blend-screen"
        style={{
          background: 'radial-gradient(circle, var(--color-accent) 0%, transparent 64%)',
          opacity: 0.16,
          filter: `blur(${perf.blur(150)}px)`,
          animation: idle ? 'star-halo 34s ease-in-out -12s infinite' : undefined,
        }}
      />
    </div>
  );
});
