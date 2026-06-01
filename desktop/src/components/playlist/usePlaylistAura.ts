import {useMemo} from 'react';
import {type Aura, auraFromHex} from '../../lib/aura';
import {parseCssColor, rgbToHex} from '../../lib/genre-aura';
import {useViewerAura} from '../../lib/useViewerAura';
import type {Track} from '../../stores/player';
import {genreColor, vibeEnergy} from '../search/utils';

export interface GenreFleck {
    genre: string;
    share: number;
    color: string;
}

export interface PlaylistAura {
    /** Top genre hues, passed as separate orbs so Atmosphere blends them. */
    tint: string[];
    /** Average energy across the crate's dominant genres. */
    energy: number;
    /** Aura from the dominant hue — title gradient, play pill, toned shadows. */
    aura: Aura;
    accentGlow: string;
    /** Top genres with their share, for the fleck legend / ribbon. */
    topGenres: GenreFleck[];
}

/** The crate's blended identity: a diverse playlist glows polychromatic, a
 *  focused one burns one hue. Recomputes as pages load (keyed on a cheap
 *  signature, not every track object). */
export function usePlaylistAura(tracks: Track[], fallbackGenre?: string | null): PlaylistAura {
    const viewer = useViewerAura();
    const sig =
        tracks.length === 0
            ? ''
            : `${tracks.length}:${tracks[0]?.urn}:${tracks[tracks.length - 1]?.urn}`;

    // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on the cheap signature
    return useMemo(() => {
        const counts = new Map<string, number>();
        let withGenre = 0;
        for (const tr of tracks) {
            const g = tr.genre?.trim();
            if (!g) continue;
            counts.set(g, (counts.get(g) ?? 0) + 1);
            withGenre++;
        }
        if (counts.size === 0 && fallbackGenre?.trim()) {
            counts.set(fallbackGenre.trim(), 1);
            withGenre = 1;
        }

        const sorted = [...counts.entries()].sort((a, b) => b[1] - a[1]);
        const top = sorted.slice(0, 3);

        if (top.length === 0) {
            const [r, g, b] = viewer.accent;
            return {
                tint: [],
                energy: 0.5,
                aura: viewer,
                accentGlow: `rgba(${r}, ${g}, ${b}, 0.32)`,
                topGenres: [],
            };
        }

        const topGenres: GenreFleck[] = top.map(([genre, n]) => ({
            genre,
            share: withGenre ? n / withGenre : 0,
            color: genreColor(genre),
        }));
        const domRgb = parseCssColor(topGenres[0].color) ?? viewer.accent;
        const [r, g, b] = domRgb;
        return {
            tint: topGenres.map((t) => t.color),
            energy: vibeEnergy(top.map(([genre]) => genre)),
            aura: auraFromHex(rgbToHex(domRgb)) ?? viewer,
            accentGlow: `rgba(${r}, ${g}, ${b}, 0.32)`,
            topGenres,
        };
    }, [sig, fallbackGenre, viewer]);
}
