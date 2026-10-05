import { memo } from 'react';
import { type Aura, auraRgb, auraRgba } from '../../lib/aura';
import { Disc3 } from '../../lib/icons';

interface AlbumCoverArtifactProps {
  title: string;
  coverUrl?: string;
  hasStar: boolean;
  aura: Aura;
  /** Retained for API compatibility; the ring no longer rotates. */
  spinning?: boolean;
  /** Size classes for the square root; defaults to the classic hero sizes. */
  sizeClassName?: string;
  /** Draw a hairline outline around the artwork. */
  outline?: boolean;
}

function AlbumCoverArtifactImpl({
  title,
  coverUrl,
  hasStar,
  aura,
  sizeClassName,
  outline,
}: AlbumCoverArtifactProps) {
  return (
    <div
      className={`relative shrink-0 self-center lg:self-start group ${
        sizeClassName ?? 'w-[180px] h-[180px] md:w-[220px] md:h-[220px]'
      }`}
    >
      {hasStar && (
        <div
          className="absolute -inset-[5px] rounded-[2.4rem] pointer-events-none overflow-hidden"
          style={{
            padding: '3px',
            WebkitMask: 'linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0)',
            WebkitMaskComposite: 'xor',
            maskComposite: 'exclude',
          }}
        >
          <div className="absolute -inset-[40%]" style={{ background: auraRgb(aura) }} />
        </div>
      )}

      <div
        className="relative w-full h-full rounded-xl overflow-hidden"
        style={{
          background: 'rgba(255,255,255,0.03)',
          boxShadow: `${
            hasStar
              ? 'inset 0 0 0 1px rgba(255,255,255,0.12), inset 0 1px 0 rgba(255,255,255,0.15)'
              : '0 25px 60px rgba(0,0,0,0.55), inset 0 0 0 1px rgba(255,255,255,0.08), inset 0 1px 0 rgba(255,255,255,0.1)'
          }${outline ? ', 0 0 0 1px rgba(255,255,255,0.18)' : ''}`,
        }}
      >
        {coverUrl ? (
          <img src={coverUrl} alt={title} className="w-full h-full object-cover" decoding="async" />
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
      </div>
    </div>
  );
}

export const AlbumCoverArtifact = memo(AlbumCoverArtifactImpl);
