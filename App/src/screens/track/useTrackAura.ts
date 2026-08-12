import { useMemo } from 'react';
import { genreColor, useScTheme } from '@sc/ui';

export interface TrackAura {
  accent: string;
  accentGlow: string;
  accentSoft: string;
  nameGradient: string;
  energy: number; // 0 спокойно .. 1 энергично
  hasGenre: boolean;
}

function hslToRgb(h: number, s: number, l: number): [number, number, number] {
  s /= 100;
  l /= 100;
  const k = (n: number) => (n + h / 30) % 12;
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => l - a * Math.max(-1, Math.min(k(n) - 3, 9 - k(n), 1));
  return [Math.round(f(0) * 255), Math.round(f(8) * 255), Math.round(f(4) * 255)];
}

function parseRgb(c: string): [number, number, number] | null {
  if (c.startsWith('#')) {
    const h = c.slice(1);
    if (h.length === 3) return [parseInt(h[0] + h[0], 16), parseInt(h[1] + h[1], 16), parseInt(h[2] + h[2], 16)];
    if (h.length >= 6) return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
    return null;
  }
  // genreColor отдаёт hsl() для жанров вне палитры
  const hsl = c.match(/hsl\(\s*([\d.]+)[,\s]+([\d.]+)%[,\s]+([\d.]+)%/i);
  if (hsl) return hslToRgb(Number(hsl[1]), Number(hsl[2]), Number(hsl[3]));
  const m = c.match(/rgba?\(([^)]+)\)/);
  if (!m) return null;
  const p = m[1].split(',').map((x) => Number.parseFloat(x));
  return [p[0], p[1], p[2]];
}

const rgba = (rgb: [number, number, number], a: number) => `rgba(${rgb[0]}, ${rgb[1]}, ${rgb[2]}, ${a})`;
const lighten = (rgb: [number, number, number], amt: number): string =>
  `rgb(${rgb.map((v) => Math.round(v + (255 - v) * amt)).join(', ')})`;

const HOT = /phonk|trap|drift|hardstyle|drum|dnb|techno|hyperpop|rage|metal|punk|hard/i;
const COLD = /ambient|lo-?fi|chill|jazz|classical|piano|acoustic|folk|soul|r&b|rnb/i;

export function useTrackAura(genre: string | null | undefined): TrackAura {
  const { accent } = useScTheme();
  return useMemo(() => {
    const g = genre?.trim();
    const base = g ? genreColor(g) : accent.base;
    const rgb = parseRgb(base) ?? [255, 45, 85];
    const energy = !g ? 0.5 : HOT.test(g) ? 0.85 : COLD.test(g) ? 0.28 : 0.55;
    const gradLight = lighten(rgb, 0.5);
    const gradMid = lighten(rgb, 0.25);
    return {
      accent: `rgb(${rgb.join(', ')})`,
      accentGlow: rgba(rgb, 0.32),
      accentSoft: rgba(rgb, 0.16),
      nameGradient: g
        ? `linear-gradient(110deg, #fff 0%, #fff 28%, ${gradLight} 45%, ${gradMid} 58%, #fff 75%, #fff 100%)`
        : 'linear-gradient(110deg, #fff 0%, #fff 30%, #d4d4d8 50%, #fff 70%, #fff 100%)',
      energy,
      hasGenre: !!g,
    };
  }, [genre, accent.base]);
}
