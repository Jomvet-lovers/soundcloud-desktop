import { memo, useCallback, useMemo } from 'react';
import { type Aura, auraRgb, auraRgba, isLight } from '../../lib/aura';
import { pauseBlack14, pauseWhite14, playBlack14, playWhite14 } from '../../lib/icons';
import { useIsPlayingFrom } from '../../lib/useTrackPlay';
import { type Track, usePlayerStore } from '../../stores/player';

interface AlbumPlayButtonProps {
  tracks: Track[];
  aura: Aura;
}

function AlbumPlayButtonImpl({ tracks, aura }: AlbumPlayButtonProps) {
  const { playable, playableUrns } = useMemo(() => {
    const list: Track[] = [];
    const urns = new Set<string>();
    for (const tr of tracks) {
      if (tr.enrichment?.availability !== 'wanted') {
        list.push(tr);
        urns.add(tr.urn);
      }
    }
    return { playable: list, playableUrns: urns };
  }, [tracks]);

  const isPlayingFromAlbum = useIsPlayingFrom(playableUrns);

  const lightAura = isLight(aura);
  const icon = isPlayingFromAlbum
    ? lightAura
      ? pauseBlack14
      : pauseWhite14
    : lightAura
      ? playBlack14
      : playWhite14;

  const empty = playable.length === 0;

  const onClick = useCallback(() => {
    if (empty) return;
    const { play, pause, resume, currentTrack } = usePlayerStore.getState();
    const currentUrn = currentTrack?.urn;
    if (isPlayingFromAlbum) {
      pause();
      return;
    }
    if (currentUrn && playableUrns.has(currentUrn)) {
      resume();
      return;
    }
    play(playable[0], playable);
  }, [empty, isPlayingFromAlbum, playable, playableUrns]);

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={empty}
      className="group relative inline-flex items-center gap-3 h-11 pl-2 pr-5 rounded-full text-[13px] font-semibold cursor-pointer transition-all duration-500 hover:scale-[1.03] active:scale-[0.97] disabled:opacity-50 disabled:cursor-not-allowed"
      style={{
        background: auraRgb(aura),
        color: lightAura ? '#000' : '#fff',
        boxShadow: `inset 0 0 0 1px ${auraRgba(aura, 0.55)}, inset 0 1px 0 rgba(255,255,255,0.3)`,
      }}
    >
      <span
        className="w-7 h-7 rounded-full flex items-center justify-center"
        style={{
          background: lightAura ? 'rgba(0,0,0,0.18)' : 'rgba(255,255,255,0.18)',
          boxShadow: `inset 0 0 0 1px ${lightAura ? 'rgba(0,0,0,0.25)' : 'rgba(255,255,255,0.25)'}`,
        }}
      >
        {icon}
      </span>
      <span className="tracking-wide">{isPlayingFromAlbum ? 'Pause Album' : 'Play Album'}</span>
    </button>
  );
}

export const AlbumPlayButton = memo(AlbumPlayButtonImpl);
