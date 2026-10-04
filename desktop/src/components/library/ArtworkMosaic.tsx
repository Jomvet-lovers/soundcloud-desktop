import { memo, useMemo } from 'react';
import { art } from '../../lib/formatters';
import { usePerfMode } from '../../lib/perf';
import type { Track } from '../../stores/player';

/** A patchwork of your taste: the covers you've liked, tiled, heavily blurred
 *  and dimmed behind the masthead. Beauty-only — the blur is a real cost, but
 *  the wall is cached as one layer and only translated. */
export const ArtworkMosaic = memo(function ArtworkMosaic({ tracks }: { tracks: Track[] }) {
  const perf = usePerfMode();
  const covers = useMemo(
    () =>
      tracks
        .map((t) => art(t.artwork_url, 't200x200'))
        .filter((u): u is string => !!u)
        .slice(0, 21),
    [tracks],
  );

  if (!perf.bloom || covers.length < 8) return null;
  const b = perf.blur(44);
  const tiles = [...covers, ...covers];

  return (
    <div
      className="absolute inset-0 overflow-hidden rounded-[inherit]"
      style={{ contain: 'strict', transform: 'translateZ(0)' }}
    >
      <div
        className="absolute -inset-[8%] flex flex-wrap content-start gap-1.5 opacity-[0.16]"
        style={{ filter: `blur(${b}px) saturate(140%)` }}
      >
        {tiles.map((src, i) => (
          <img
            key={`${i}-${src}`}
            src={src}
            alt=""
            decoding="async"
            className="w-[13%] aspect-square object-cover rounded-2xl"
          />
        ))}
      </div>
    </div>
  );
});
