import React from 'react';
import type { Aura } from '../../lib/aura';
import { AlbumCoverArtifact } from '../album/AlbumCoverArtifact';

/** The room's light source: the cover artifact (genre-ring when verified).
 *  Purely decorative — playback lives on the hero's play button. */
export const TrackCover = React.memo(function TrackCover({
  title,
  coverUrl,
  aura,
  verified,
  sizeClassName,
}: {
  title: string;
  coverUrl?: string;
  aura: Aura;
  verified: boolean;
  sizeClassName?: string;
}) {
  return (
    <div className="relative shrink-0 self-center lg:self-start">
      <AlbumCoverArtifact
        title={title}
        coverUrl={coverUrl}
        hasStar={verified}
        aura={aura}
        sizeClassName={sizeClassName}
        outline
      />
    </div>
  );
});
