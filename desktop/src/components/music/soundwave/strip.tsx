import React from 'react';
import type { Track } from '../../../stores/player';
import { HorizontalScroll } from '../../ui/HorizontalScroll';
import { TrackCard } from '../TrackCard';

interface StripProps {
  tracks: Track[];
  /** Card width in px. */
  width?: number;
}

export const RecommendationsStrip = React.memo(function RecommendationsStrip({
  tracks,
  width = 180,
}: StripProps) {
  return (
    <HorizontalScroll>
      {tracks.map((track) => (
        <div key={track.urn} className="shrink-0" style={{ width }}>
          <TrackCard track={track} queue={tracks} />
        </div>
      ))}
    </HorizontalScroll>
  );
});
