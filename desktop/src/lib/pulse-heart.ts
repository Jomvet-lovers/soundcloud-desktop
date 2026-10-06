import { useCallback, useEffect, useRef, useState } from 'react';

/** ReactBits' "Pulse Heart" timing: the heart contracts to a dot, the colour
 *  flips at the smallest frame, then it springs back with an overshoot. */
const DURATION = 560;
const OUT = 0.4;
const DOT = 0.3;
const OVERSHOOT = 1.7;
const BEAT = 0.03;

/** Back-out easing past the smallest frame (overshoot > 1 springs past 1). */
function back(k: number, c: number): number {
  const u = k - 1;
  return 1 + (c + 1) * u ** 3 + c * u ** 2;
}

function swellOf(t: number, c: number): number {
  if (t <= 0) return 0;
  if (t < OUT) return 1 - (1 - t / OUT) ** 3;
  return 1 - back((t - OUT) / (1 - OUT), c);
}

function reducedMotion(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!window.matchMedia?.('(prefers-reduced-motion: reduce)').matches
  );
}

/** Drives a like button's pulse. `shownLiked` lags the logical state by half
 *  the animation so the colour flips at the smallest frame, like the original.
 *  Attach `pillRef` to the button and `heartRef` to a span around the icon. */
export function usePulseHeart(liked: boolean) {
  const [shownLiked, setShownLiked] = useState(liked);
  const heartRef = useRef<HTMLElement | null>(null);
  const pillRef = useRef<HTMLElement | null>(null);
  const raf = useRef(0);

  useEffect(() => {
    if (!raf.current) setShownLiked(liked);
  }, [liked]);

  useEffect(() => () => cancelAnimationFrame(raf.current), []);

  const pulse = useCallback((nextLiked: boolean) => {
    const heart = heartRef.current;
    if (!heart || reducedMotion()) {
      setShownLiked(nextLiked);
      return;
    }
    cancelAnimationFrame(raf.current);
    const t0 = performance.now();
    let swapped = false;
    const tick = (now: number) => {
      const t = Math.min(1, (now - t0) / DURATION);
      const s = swellOf(t, OVERSHOOT);
      heart.style.transform = `scale(${1 - (1 - DOT) * s})`;
      if (pillRef.current) pillRef.current.style.transform = `scale(${1 - BEAT * s})`;
      if (!swapped && t >= OUT) {
        swapped = true;
        setShownLiked(nextLiked);
      }
      if (t < 1) {
        raf.current = requestAnimationFrame(tick);
        return;
      }
      raf.current = 0;
      heart.style.transform = '';
      if (pillRef.current) pillRef.current.style.transform = '';
    };
    raf.current = requestAnimationFrame(tick);
  }, []);

  return { shownLiked, pulse, heartRef, pillRef };
}
