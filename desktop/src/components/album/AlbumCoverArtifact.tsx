import { memo } from 'react';
import { type Aura, auraRgb, auraRgba } from '../../lib/aura';
import { Disc3 } from '../../lib/icons';
import { usePerfMode } from '../../lib/perf';

interface AlbumCoverArtifactProps {
  title: string;
  coverUrl?: string;
  hasStar: boolean;
  aura: Aura;
  /** Rotate the ring. Defaults to true (idle spin); pass `isPlaying` to spin only on playback. */
  spinning?: boolean;
}

function AlbumCoverArtifactImpl({
  title,
  coverUrl,
  hasStar,
  aura,
  spinning = true,
}: AlbumCoverArtifactProps) {
  const { idleAnim } = usePerfMode();
  return (
    <div className="relative shrink-0 self-center lg:self-start group w-[180px] h-[180px] md:w-[220px] md:h-[220px]">
      {hasStar && (
        <div
          className="absolute -inset-[5px] rounded-[2.4rem] pointer-events-none overflow-hidden"
          style={{
            padding: '3px',
            WebkitMask: 'linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)',
            WebkitMaskComposite: 'xor',
            maskComposite: 'exclude',
            filter: `drop-shadow(0 0 14px ${aura.orbs[0]}aa)`,
          }}
        >
          <div
            className="absolute -inset-[40%]"
            style={{
              background: auraRgb(aura),
              animation: idleAnim && spinning ? 'ring-rotate 12s linear infinite' : undefined,
            }}
          />
        </div>
      )}

      <div
        className="relative w-full h-full rounded-[2.2rem] overflow-hidden"
        style={{
          background: 'rgba(255,255,255,0.03)',
          boxShadow: hasStar
            ? 'inset 0 0 0 1px rgba(255,255,255,0.12), inset 0 1px 0 rgba(255,255,255,0.15)'
            : '0 25px 60px rgba(0,0,0,0.55), inset 0 0 0 1px rgba(255,255,255,0.08), inset 0 1px 0 rgba(255,255,255,0.1)',
        }}
      >
        {coverUrl ? (
          <img
            src={coverUrl}
            alt={title}
            className="w-full h-full object-cover transition-transform duration-1000 group-hover:scale-[1.06]"
            decoding="async"
          />
        ) : (
          <div
            className="w-full h-full flex items-center justify-center"
            style={{
              background: hasStar ? auraRgba(aura, 0.2) : 'rgba(255,255,255,0.03)',
            }}
          >
            <Disc3 size={72} className="text-white/15" />
          </div>
        )}

        <div className="absolute inset-x-0 top-0 h-1/2 pointer-events-none bg-gradient-to-b from-white/10 to-transparent mix-blend-overlay" />
      </div>
    </div>
  );
}

export const AlbumCoverArtifact = memo(AlbumCoverArtifactImpl);
