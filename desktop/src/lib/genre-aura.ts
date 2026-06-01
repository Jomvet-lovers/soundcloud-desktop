import {hexToRgb} from './aura';

export type Rgb = [number, number, number];

function hslToRgb(h: number, s: number, l: number): Rgb {
    const hh = (((h % 360) + 360) % 360) / 360;
    if (s === 0) {
        const v = Math.round(l * 255);
        return [v, v, v];
    }
    const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
    const p = 2 * l - q;
    const ch = (t: number) => {
        let x = t;
        if (x < 0) x += 1;
        if (x > 1) x -= 1;
        if (x < 1 / 6) return p + (q - p) * 6 * x;
        if (x < 1 / 2) return q;
        if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
        return p;
    };
    return [
        Math.round(ch(hh + 1 / 3) * 255),
        Math.round(ch(hh) * 255),
        Math.round(ch(hh - 1 / 3) * 255),
    ];
}

/** genreColor() yields either a #hex (curated genres) or an hsl() (hashed). */
export function parseCssColor(c: string): Rgb | null {
    if (c.startsWith('#')) return hexToRgb(c);
    const m = c.match(/hsl\(\s*([\d.]+)[,\s]+([\d.]+)%[,\s]+([\d.]+)%/i);
    if (m) return hslToRgb(+m[1], +m[2] / 100, +m[3] / 100);
    return null;
}

export const rgbToHex = ([r, g, b]: Rgb): string =>
    `#${[r, g, b].map((n) => Math.round(n).toString(16).padStart(2, '0')).join('')}`;

export const rgbCss = ([r, g, b]: Rgb) => `rgb(${r}, ${g}, ${b})`;
export const rgbaCss = ([r, g, b]: Rgb, a: number) => `rgba(${r}, ${g}, ${b}, ${a})`;
