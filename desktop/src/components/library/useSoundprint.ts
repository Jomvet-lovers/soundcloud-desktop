import {useMemo} from 'react';
import {type Aura, auraFromHex} from '../../lib/aura';
import {parseCssColor, rgbToHex} from '../../lib/genre-aura';
import {useViewerAura} from '../../lib/useViewerAura';
import type {Track} from '../../stores/player';
import {type GenreShare, genreColor, topGenres, vibeEnergy} from '../search/utils';

export interface Soundprint {
    /** The dominant genres of the collection — the spectrum's columns. */
    spectrum: GenreShare[];
    /** Top hues for the page Atmosphere to blend. */
    tint: string[];
    energy: number;
    /** The page identity hue: the viewer's accent by default, the picked genre tag
     *  when one is selected. */
    aura: Aura;
    accentGlow: string;
    hasData: boolean;
}

/** Your taste, made measurable: the genres you've actually liked, ranked. The
 *  page wears YOUR accent colour by default; picking a genre tag retints the whole
 *  room to that genre instead. */
export function useSoundprint(tracks: Track[], selectedGenre?: string | null, n = 7): Soundprint {
    const viewer = useViewerAura();
    const sig = tracks.length === 0 ? '' : `${tracks.length}:${tracks[0]?.urn}`;

    // biome-ignore lint/correctness/useExhaustiveDependencies: keyed on the cheap signature
    return useMemo(() => {
        const spectrum = topGenres(tracks, n);
        // Default identity = the viewer's accent; a selected genre tag overrides it.
        const selRgb = selectedGenre ? parseCssColor(genreColor(selectedGenre)) : null;
        const aura = selRgb ? (auraFromHex(rgbToHex(selRgb)) ?? viewer) : viewer;
        const [r, g, b] = aura.accent;
        return {
            spectrum,
            tint: [...aura.orbs],
            energy: spectrum.length ? vibeEnergy(spectrum.map((s) => s.genre)) : 0.5,
            aura,
            accentGlow: `rgba(${r}, ${g}, ${b}, 0.32)`,
            hasData: spectrum.length > 0,
        };
    }, [sig, n, viewer, selectedGenre]);
}
