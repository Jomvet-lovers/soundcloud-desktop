import {useMemo} from 'react';
import {type Aura, auraFromHex} from '../../lib/aura';
import {parseCssColor, rgbToHex} from '../../lib/genre-aura';
import {useViewerAura} from '../../lib/useViewerAura';
import {genreColor, genreEnergy} from '../search/utils';

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
        const rgb = (g ? parseCssColor(genreColor(g)) : null) ?? viewer.accent;
        const [r, gg, b] = rgb;
        return {
            aura: g ? (auraFromHex(rgbToHex(rgb)) ?? viewer) : viewer,
            tint: g ? [`rgb(${r}, ${gg}, ${b})`] : undefined,
            energy: g ? genreEnergy(g) : 0.5,
            accent: `rgb(${r}, ${gg}, ${b})`,
            accentGlow: `rgba(${r}, ${gg}, ${b}, 0.32)`,
            accentSoft: `rgba(${r}, ${gg}, ${b}, 0.16)`,
            hasGenre: !!g,
        };
    }, [genre, viewer]);
}
