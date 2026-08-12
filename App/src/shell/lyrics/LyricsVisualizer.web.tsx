import { createElement, useEffect, useRef } from 'react';
import { useSc } from '@sc/data';
import { useScTheme } from '@sc/ui';

/** Полноэкранная FFT-волна лирики (донор `LyricsVisualizer`) — canvas на web, живёт
 *  от события ядра `spectrum` (~64 лог-полосы, ~30 Гц; FFT в ядре крутится только пока
 *  есть подписчик). Никакого поллинга: перерисовка на новый кадр + короткий decay-хвост,
 *  rAF паркуется в простое. */
const VIS_BINS = 64;

function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '');
  const v = h.length === 3 ? h.split('').map((c) => c + c).join('') : h;
  return [parseInt(v.slice(0, 2), 16), parseInt(v.slice(2, 4), 16), parseInt(v.slice(4, 6), 16)] as [number, number, number];
}

export function LyricsVisualizer() {
  const sc = useSc();
  const { accent } = useScTheme();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const wrapRef = useRef<HTMLDivElement>(null);
  const target = useRef(new Float32Array(VIS_BINS));
  const display = useRef(new Float32Array(VIS_BINS));

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap) return;
    const ctx = canvas.getContext('2d', { alpha: true });
    if (!ctx) return;
    const rgb = hexToRgb(accent.base);
    let cssW = 0;
    let cssH = 0;

    const resize = () => {
      const r = wrap.getBoundingClientRect();
      cssW = Math.max(1, Math.floor(r.width));
      cssH = Math.max(1, Math.floor(r.height));
      const dpr = Math.min(window.devicePixelRatio || 1, 2);
      canvas.width = Math.floor(cssW * dpr);
      canvas.height = Math.floor(cssH * dpr);
      canvas.style.width = `${cssW}px`;
      canvas.style.height = `${cssH}px`;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();
    const ro = new ResizeObserver(resize);
    ro.observe(wrap);

    const smoothed = new Float32Array(VIS_BINS);
    const draw = () => {
      if (document.visibilityState === 'hidden') return;
      ctx.clearRect(0, 0, cssW, cssH);
      const d = display.current;
      // 1-2-1 сглаживание между полосами — убирает лесенку низы→верхи.
      smoothed[0] = (d[0] * 3 + d[1]) * 0.25;
      smoothed[VIS_BINS - 1] = (d[VIS_BINS - 1] * 3 + d[VIS_BINS - 2]) * 0.25;
      for (let i = 1; i < VIS_BINS - 1; i++) smoothed[i] = d[i - 1] * 0.25 + d[i] * 0.5 + d[i + 1] * 0.25;

      const baseY = cssH - 6;
      const maxAmp = cssH * 0.78;
      let peak = 0;
      for (let i = 0; i < VIS_BINS; i++) if (d[i] > peak) peak = d[i];

      const trace = (amp: number) => {
        ctx.beginPath();
        ctx.moveTo(0, baseY - smoothed[0] * maxAmp * amp);
        for (let i = 0; i < VIS_BINS - 1; i++) {
          const xA = (i / (VIS_BINS - 1)) * cssW;
          const xB = ((i + 1) / (VIS_BINS - 1)) * cssW;
          const yA = baseY - smoothed[i] * maxAmp * amp;
          const yB = baseY - smoothed[i + 1] * maxAmp * amp;
          ctx.quadraticCurveTo(xA, yA, (xA + xB) * 0.5, (yA + yB) * 0.5);
        }
        ctx.lineTo(cssW, baseY - smoothed[VIS_BINS - 1] * maxAmp * amp);
      };

      const g = ctx.createLinearGradient(0, 0, 0, cssH);
      const a = Math.min(1, peak * 1.3);
      g.addColorStop(0, `rgba(${rgb[0]},${rgb[1]},${rgb[2]},0)`);
      g.addColorStop(0.5, `rgba(${rgb[0]},${rgb[1]},${rgb[2]},${(0.18 * a).toFixed(3)})`);
      g.addColorStop(1, `rgba(${rgb[0]},${rgb[1]},${rgb[2]},${(0.32 * a).toFixed(3)})`);
      trace(1);
      ctx.lineTo(cssW, baseY);
      ctx.lineTo(0, baseY);
      ctx.closePath();
      ctx.fillStyle = g;
      ctx.fill();

      const stroke = (amp: number, alphaMul: number, useAccent: boolean, lw: number) => {
        trace(amp);
        const [r, gg, b] = useAccent ? rgb : [255, 255, 255];
        const alpha = (0.45 + 0.4 * Math.min(1, peak)) * alphaMul;
        ctx.lineCap = 'round';
        ctx.lineJoin = 'round';
        ctx.lineWidth = lw;
        ctx.strokeStyle = `rgba(${r},${gg},${b},${alpha.toFixed(3)})`;
        ctx.shadowBlur = 24 * alphaMul;
        ctx.shadowColor = `rgba(${r},${gg},${b},${(alpha * 0.6).toFixed(3)})`;
        ctx.stroke();
      };
      stroke(1, 1, true, 2.4);
      stroke(0.78, 0.5, false, 1.2);
      ctx.shadowBlur = 0;
    };

    let rafId = 0;
    let lastEventTs = 0;
    let lastTs = performance.now();
    let dirty = false;
    const loop = (ts: number) => {
      const dt = Math.min(0.05, (ts - lastTs) / 1000);
      lastTs = ts;
      const t = target.current;
      const d = display.current;
      const attack = 1 - Math.exp(-dt * 18);
      const release = 1 - Math.exp(-dt * 5);
      let any = false;
      for (let i = 0; i < VIS_BINS; i++) {
        const k = t[i] > d[i] ? attack : release;
        d[i] += (t[i] - d[i]) * k;
        if (d[i] > 1e-3 || t[i] > 1e-3) any = true;
      }
      draw();
      if (any && (dirty || ts - lastEventTs < 350)) rafId = requestAnimationFrame(loop);
      else {
        rafId = 0;
        dirty = false;
      }
    };
    const ensureLoop = () => {
      if (!rafId) rafId = requestAnimationFrame(loop);
    };

    const off = sc.on('spectrum', (bins) => {
      if (!bins || bins.length === 0) return;
      const t = target.current;
      const n = Math.min(t.length, bins.length);
      for (let i = 0; i < n; i++) t[i] = bins[i];
      lastEventTs = performance.now();
      dirty = true;
      ensureLoop();
    });

    return () => {
      if (rafId) cancelAnimationFrame(rafId);
      ro.disconnect();
      off();
    };
  }, [sc, accent.base]);

  return createElement(
    'div',
    {
      ref: wrapRef,
      style: {
        position: 'absolute',
        left: 0,
        right: 0,
        bottom: 0,
        height: 'clamp(320px, 62vh, 100vh)',
        zIndex: 0,
        pointerEvents: 'none',
        maskImage: 'linear-gradient(to top, black 0%, black 60%, transparent 100%)',
        WebkitMaskImage: 'linear-gradient(to top, black 0%, black 60%, transparent 100%)',
        contain: 'strict',
        transform: 'translateZ(0)',
      },
    },
    createElement('canvas', { ref: canvasRef, style: { display: 'block' } }),
  );
}
