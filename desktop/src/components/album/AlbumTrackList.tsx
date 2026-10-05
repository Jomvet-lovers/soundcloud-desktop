import { memo, useMemo } from 'react';
import type { Aura } from '../../lib/aura';
import { dur, fc } from '../../lib/formatters';
import { ListMusic, Music } from '../../lib/icons';
import { useArtistDisplay, useDisplayTitle } from '../../lib/track-display';
import type { Track } from '../../stores/player';
import { AlbumTrackRow } from './AlbumTrackRow';

interface AlbumTrackListProps {
  tracks: Track[];
  aura: Aura;
}

interface Partitioned {
  available: Track[];
  wanted: Track[];
  totalDuration: number;
}

function partition(tracks: Track[]): Partitioned {
  const available: Track[] = [];
  const wanted: Track[] = [];
  let totalDuration = 0;
  for (const t of tracks) {
    if (t.enrichment?.availability === 'wanted') {
      wanted.push(t);
    } else {
      available.push(t);
      totalDuration += t.duration ?? 0;
    }
  }
  return { available, wanted, totalDuration };
}

const WantedRow = memo(function WantedRow({ track, position }: { track: Track; position: number }) {
  const displayTitle = useDisplayTitle(track);
  const artistDisplay = useArtistDisplay(track);
  return (
    <div
      className="flex items-center gap-4 px-4 py-2.5 rounded-2xl opacity-50"
      style={{ background: 'rgba(255,255,255,0.015)' }}
    >
      <div className="w-9 h-9 flex items-center justify-center shrink-0">
        <span className="text-[13px] text-white/25 tabular-nums font-semibold">{position}</span>
      </div>
      <div
        className="w-9 h-9 rounded-lg flex items-center justify-center shrink-0"
        style={{
          background: 'rgba(255,255,255,0.03)',
          boxShadow: 'inset 0 0 0 1px rgba(255,255,255,0.06)',
        }}
      >
        <Music size={14} className="text-white/20" />
      </div>
      <div className="flex-1 min-w-0">
        <p className="text-[13px] font-medium text-white/55 truncate">{displayTitle}</p>
        <p className="text-[11px] text-white/25 truncate">{artistDisplay.primary}</p>
      </div>
      {track.duration ? (
        <span className="text-[11px] text-white/25 tabular-nums shrink-0 w-12 text-right">
          {dur(track.duration)}
        </span>
      ) : (
        <span className="text-[11px] text-white/15 shrink-0 w-12 text-right">—</span>
      )}
    </div>
  );
});

function AlbumTrackListImpl({ tracks, aura }: AlbumTrackListProps) {
  const { available, wanted, totalDuration } = useMemo(() => partition(tracks), [tracks]);

  if (tracks.length === 0) {
    return (
      <div className="py-24 flex flex-col items-center gap-3">
        <Music size={28} className="text-white/15" />
        <p className="text-white/30 text-sm">{'No tracks indexed yet'}</p>
      </div>
    );
  }

  return (
    <section className="flex flex-col gap-1">
      <div className="flex items-center justify-between px-1 pb-3">
        <span className="inline-flex items-center gap-2 text-[12px] font-medium text-white/60">
          <ListMusic size={12} /> {'Tracks'}
          <span className="ml-1 text-white/30">{available.length}</span>
        </span>
        <span className="text-[11px] tabular-nums text-white/30">{dur(totalDuration)}</span>
      </div>

      {available.length > 0 && (
        <div className="flex flex-col gap-1">
          {available.map((track, i) => (
            <AlbumTrackRow
              key={track.urn}
              track={track}
              position={i + 1}
              queue={available}
              aura={aura}
            />
          ))}
        </div>
      )}

      {wanted.length > 0 && (
        <div className="mt-6 space-y-3">
          <div className="flex items-center gap-3 px-1">
            <span className="text-[10px] font-medium text-white/45">{'Coming soon'}</span>
            <span className="text-[11px] tabular-nums text-white/30">{fc(wanted.length)}</span>
            <div className="h-px flex-1 bg-white/[0.05]" />
          </div>
          <div className="flex flex-col gap-1">
            {wanted.map((track, i) => (
              <WantedRow key={track.urn} track={track} position={available.length + i + 1} />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}

export const AlbumTrackList = memo(AlbumTrackListImpl);
