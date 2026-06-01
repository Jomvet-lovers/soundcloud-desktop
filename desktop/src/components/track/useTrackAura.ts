import {useMemo} from 'react';
import {type Aura, auraFromHex, hexToRgb} from '../../lib/aura';
import {useViewerAura} from '../../lib/useViewerAura';
import {genreColor, genreEnergy} from '../search/utils';

type Rgb = [number, number, number];

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
function parseColor(c: string): Rgb | null {
    if (c.startsWith('#')) return hexToRgb(c);
    const m = c.match(/hsl\(\s*([\d.]+)[,\s]+([\d.]+)%[,\s]+([\d.]+)%/i);
    if (m) return hslToRgb(+m[1], +m[2] / 100, +m[3] / 100);
    return null;
}

const toHex = ([r, g, b]: Rgb) =>
    `#${[r, g, b].map((n) => Math.round(n).toString(16).padStart(2, '0')).join('')}`;

export interface TrackAura {
    /** Full aura (orbs + accent + nameGradient) for the cover ring & toned shadows. */
    aura: Aura;
    /** Atmosphere tint — the genre hue, or undefined to let it fall back to accent. */
    tint: string[] | undefined;
    /** 0 (calm/cold) .. 1 (hot/fast) — the room's drift tempo. */
    energy: number;
    /** The track's own color, for the scoped --color-accent override on the wave. */
    accent: string;
    accentGlow: string;
    /** A softer fill (~0.16α) for chips/badges that should carry the genre hue. */
    accentSoft: string;
    hasGenre: boolean;
}

/** The room's identity, derived once from the track's genre. Feeds the
 *  Atmosphere, the cover ring, the waveform's color and toned accents. */
export function useTrackAura(genre: string | null | undefined): TrackAura {
    const viewer = useViewerAura();
    return useMemo(() => {
        const g = genre?.trim();
        if (!g) {
            const [r, gg, b] = viewer.accent;
            return {
                aura: viewer,
                tint: undefined,
                energy: 0.5,
                accent: `rgb(${r}, ${gg}, ${b})`,
                accentGlow: `rgba(${r}, ${gg}, ${b}, 0.32)`,
                accentSoft: `rgba(${r}, ${gg}, ${b}, 0.16)`,
                hasGenre: false,
            };
        }
        const rgb = parseColor(genreColor(g)) ?? viewer.accent;
        const [r, gg, b] = rgb;
        return {
            aura: auraFromHex(toHex(rgb)) ?? viewer,
            tint: [`rgb(${r}, ${gg}, ${b})`],
            energy: genreEnergy(g),
            accent: `rgb(${r}, ${gg}, ${b})`,
            accentGlow: `rgba(${r}, ${gg}, ${b}, 0.32)`,
            accentSoft: `rgba(${r}, ${gg}, ${b}, 0.16)`,
            hasGenre: true,
        };
    }, [genre, viewer]);
}
