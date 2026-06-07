import {trackedInvoke as invoke} from './diagnostics';

/** Online wallpaper search. The actual HTTP + JSON normalization lives in Rust
 *  (`network::wallpapers`) — Wallhaven and Konachan 403 a non-browser
 *  User-Agent, which the webview's plugin-http can't reliably set, so reqwest
 *  owns it. This module is just types, source metadata, and the invoke. */

export type WallpaperSource = 'wallhaven' | 'pinterest' | 'konachan' | 'safebooru';
export type WallpaperCategory = 'anime' | 'general' | 'people';

export interface WallpaperHit {
    id: string;
    thumb: string;
    full: string;
    resolution: string;
}

export interface WallpaperSearchResult {
    items: WallpaperHit[];
    /** Opaque token for the next page, or null when exhausted. */
    cursor: string | null;
}

export interface SourceCaps {
    /** Wallhaven-style general/anime/people split. */
    category: boolean;
    /** Colour-scheme filter. */
    color: boolean;
    /** Tag-based booru engine (the query is space-separated tags). */
    tagBased: boolean;
    /** Can surface adult content at all. */
    adult: boolean;
    /** Adult content requires a personal API key (Wallhaven). */
    adultNeedsKey: boolean;
}

export const WALLPAPER_SOURCES: ReadonlyArray<{
    id: WallpaperSource;
    label: string;
    caps: SourceCaps;
}> = [
    {
        id: 'wallhaven',
        label: 'Wallhaven',
        caps: {category: true, color: true, tagBased: false, adult: true, adultNeedsKey: true},
    },
    {
        id: 'pinterest',
        label: 'Pinterest',
        caps: {category: false, color: false, tagBased: false, adult: false, adultNeedsKey: false},
    },
    {
        id: 'konachan',
        label: 'Konachan',
        caps: {category: false, color: false, tagBased: true, adult: true, adultNeedsKey: false},
    },
    {
        id: 'safebooru',
        label: 'Safebooru',
        caps: {category: false, color: false, tagBased: true, adult: false, adultNeedsKey: false},
    },
];

export function sourceCaps(source: WallpaperSource): SourceCaps {
    return WALLPAPER_SOURCES.find((s) => s.id === source)?.caps ?? WALLPAPER_SOURCES[0].caps;
}

/** Wallhaven's fixed colour palette (the only values the `colors` param accepts). */
export const WALLPAPER_COLORS = [
    '660000',
    '990000',
    'cc0000',
    'cc3333',
    'ea4c88',
    '993399',
    '663399',
    '333399',
    '0066cc',
    '0099cc',
    '66cccc',
    '77cc33',
    '669900',
    '336600',
    '666600',
    '999900',
    'cccc33',
    'ffff00',
    'ffcc33',
    'ff9900',
    'ff6600',
    'cc6633',
    '996633',
    '663300',
    '000000',
    '999999',
    'cccccc',
    'ffffff',
    '424153',
];

export interface WallpaperQuery {
    source?: WallpaperSource;
    query?: string;
    category?: WallpaperCategory;
    color?: string | null;
    /** Opaque token from a previous result; omit/null for the first page. */
    cursor?: string | null;
    /** Opt into adult content where the source/key allows it. */
    adult?: boolean;
    /** Personal Wallhaven API key (unlocks sketchy/NSFW). */
    apiKey?: string;
}

export async function searchWallpapers(opts: WallpaperQuery): Promise<WallpaperSearchResult> {
    const res = await invoke<WallpaperSearchResult>('wallpaper_search', {
        args: {
            source: opts.source ?? 'wallhaven',
            query: opts.query ?? '',
            category: opts.category ?? null,
            color: opts.color ?? null,
            cursor: opts.cursor ?? null,
            adult: opts.adult ?? false,
            apiKey: opts.apiKey ?? null,
        },
    });
    // Don't trust the bridge blindly — normalize defensively.
    return {
        items: Array.isArray(res?.items) ? res.items : [],
        cursor: typeof res?.cursor === 'string' ? res.cursor : null,
    };
}
