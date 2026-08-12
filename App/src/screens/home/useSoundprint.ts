import { useMemo } from 'react';
import { type Track } from '@sc/data';
import {
  type Aura,
  auraFromHex,
  type GenreShare,
  genreColor,
  topGenres,
  useScTheme,
} from '@sc/ui';

export interface Soundprint {
  /** доминирующие жанры коллекции — колонки спектра */
  spectrum: GenreShare[];
  /** оттенки для атмосферы страницы */
  tint: string[];
  energy: number;
  /** идентичность страницы: акцент зрителя по умолчанию, цвет жанра при выборе */
  aura: Aura;
  accentGlow: string;
  hasData: boolean;
}

/** Твой вкус в цифрах: жанры, которые ты лайкал, по частоте. Страница носит ТВОЙ
 *  акцент; выбор жанра-тега перекрашивает всю комнату в этот жанр. */
export function useSoundprint(tracks: Track[], selectedGenre: string | null, n = 7): Soundprint {
  const { accent } = useScTheme();
  const viewer = useMemo(() => auraFromHex(accent.base) ?? auraFromHex('#ff2d55'), [accent.base]);
  const sig = tracks.length === 0 ? '' : `${tracks.length}:${tracks[0]?.id}`;

  // eslint-disable-next-line react-hooks/exhaustive-deps
  return useMemo(() => {
    const base = viewer!;
    const spectrum = topGenres(
      tracks.map((t) => t.genre),
      n,
    );
    const selHex = selectedGenre ? cssColorToHex(genreColor(selectedGenre)) : null;
    const aura = (selHex && auraFromHex(selHex)) || base;
    const [r, g, b] = aura.accent;
    return {
      spectrum,
      tint: [...aura.orbs],
      energy: spectrum.length ? 0.45 + Math.min(0.4, spectrum.length * 0.05) : 0.5,
      aura,
      accentGlow: `rgba(${r}, ${g}, ${b}, 0.32)`,
      hasData: spectrum.length > 0,
    };
  }, [sig, n, viewer, selectedGenre]);
}

/** genreColor даёт hex/hsl/var — приводим к hex для auraFromHex (var → null). */
function cssColorToHex(c: string): string | null {
  if (c.startsWith('#')) return c;
  const hsl = /^hsl\((\d+),\s*(\d+)%,\s*(\d+)%\)$/.exec(c);
  if (!hsl) return null;
  return hslToHex(Number(hsl[1]), Number(hsl[2]) / 100, Number(hsl[3]) / 100);
}

function hslToHex(h: number, s: number, l: number): string {
  const c = (1 - Math.abs(2 * l - 1)) * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = l - c / 2;
  const [r, g, b] = (
    h < 60
      ? [c, x, 0]
      : h < 120
        ? [x, c, 0]
        : h < 180
          ? [0, c, x]
          : h < 240
            ? [0, x, c]
            : h < 300
              ? [x, 0, c]
              : [c, 0, x]
  ).map((v) => Math.round((v + m) * 255));
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
}
