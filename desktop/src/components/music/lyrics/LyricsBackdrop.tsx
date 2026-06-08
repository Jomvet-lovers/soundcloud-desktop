import React, {useEffect, useRef} from 'react';
import {getWallpaperUrl} from '../../../lib/cache';
import {art} from '../../../lib/formatters';
import {usePerfMode} from '../../../lib/perf';
import {useSettingsStore} from '../../../stores/settings';

/* ── Dominant-colour extraction from the artwork ───────────── */

function extractColor(src: string): Promise<[number, number, number]> {
    return new Promise((resolve) => {
        const img = new Image();
        img.crossOrigin = 'anonymous';
        img.onload = () => {
            try {
                const c = document.createElement('canvas');
                c.width = 10;
                c.height = 10;
                const ctx = c.getContext('2d');
                if (!ctx) {
                    resolve([255, 85, 0]);
                    return;
                }
                ctx.drawImage(img, 0, 0, 10, 10);
                const d = ctx.getImageData(0, 0, 10, 10).data;
                let r = 0;
                let g = 0;
                let b = 0;
                const n = d.length / 4;
                for (let i = 0; i < d.length; i += 4) {
                    r += d[i];
                    g += d[i + 1];
                    b += d[i + 2];
                }
                resolve([Math.round(r / n), Math.round(g / n), Math.round(b / n)]);
            } catch {
                resolve([255, 85, 0]);
            }
        };
        img.onerror = () => resolve([255, 85, 0]);
        img.src = src;
    });
}

export function useArtworkColor(artworkUrl: string | null) {
    const colorRef = useRef<[number, number, number]>([255, 85, 0]);
    const prevArtRef = useRef<string | null>(null);

    useEffect(() => {
        const src = art(artworkUrl, 't200x200');
        if (!src || src === prevArtRef.current) return;
        prevArtRef.current = src;
        extractColor(src).then((c) => {
            colorRef.current = c;
        });
    }, [artworkUrl]);

    return colorRef;
}

/* ── Immersive backdrop ────────────────────────────────────────
 * Priority: the user's wallpaper (softly blurred, so the lyrics screen lives in
 * the same world as the rest of the app) → else the track artwork → else a flat
 * colour glow. The track's dominant colour always blooms on top as accent, and
 * a vertical veil keeps the lyrics legible. */

export const LyricsBackdrop = React.memo(
    ({artworkSrc, color}: { artworkSrc: string | null; color: [number, number, number] }) => {
        const perf = usePerfMode();
        const bgName = useSettingsStore((s) => s.backgroundImage);
        const wallpaperUrl = bgName ? getWallpaperUrl(bgName) : null;
        const blur = perf.blur(wallpaperUrl ? 52 : 72);
        const [r, g, b] = color;

        return (
            <div
                className="absolute inset-0 pointer-events-none"
                style={{contain: 'strict', transform: 'translateZ(0)'}}
            >
                {wallpaperUrl ? (
                    <img
                        src={wallpaperUrl}
                        alt=""
                        className="w-full h-full object-cover"
                        style={{
                            filter: blur > 0 ? `blur(${blur}px) saturate(1.12)` : undefined,
                            opacity: 0.78,
                            transform: 'scale(1.08) translateZ(0)',
                        }}
                        loading="eager"
                        decoding="async"
                    />
                ) : artworkSrc && blur > 0 ? (
                    <img
                        src={artworkSrc}
                        alt=""
                        className="w-full h-full object-cover scale-[1.2] opacity-30"
                        style={{filter: `blur(${blur}px) saturate(1.2)`}}
                        loading="eager"
                        decoding="async"
                    />
                ) : (
                    <div
                        className="absolute inset-0"
                        style={{
                            background: `
                radial-gradient(ellipse at 25% 50%, rgba(${r},${g},${b},0.2) 0%, transparent 60%),
                radial-gradient(ellipse at 75% 70%, rgba(${r},${g},${b},0.12) 0%, transparent 50%)
              `,
                        }}
                    />
                )}

                {/* track-colour bloom — ties the song's hue into the scene */}
                <div
                    className="absolute inset-0"
                    style={{
                        background: `
              radial-gradient(60% 55% at 26% 28%, rgba(${r},${g},${b},0.30) 0%, transparent 60%),
              radial-gradient(55% 55% at 80% 78%, rgba(${r},${g},${b},0.18) 0%, transparent 60%)
            `,
                    }}
                />

                {/* readability veil — lighter when there's a wallpaper so it stays visible */}
                <div
                    className="absolute inset-0"
                    style={{
                        background: wallpaperUrl
                            ? 'linear-gradient(180deg, rgba(8,8,10,0.42) 0%, rgba(8,8,10,0.5) 46%, rgba(8,8,10,0.72) 100%)'
                            : 'linear-gradient(180deg, rgba(8,8,10,0.3) 0%, rgba(8,8,10,0.56) 48%, rgba(8,8,10,0.84) 100%)',
                    }}
                />
            </div>
        );
    },
);
