import React, {useMemo} from 'react';
import {ClusterFeedbackProvider} from '../../../lib/recsFeedback';
import type {Track} from '../../../stores/player';
import {HorizontalScroll} from '../../ui/HorizontalScroll';
import {TrackCard} from '../TrackCard';
import {ClusterHeader} from './ClusterHeader';
import type {ClusterId} from './types';

interface Props {
  title: string;
  description: string;
  icon: React.ReactNode;
  index: number;
  tracks: Track[];
  queue: Track[];
  clusterId: ClusterId | string;
  cardWidth?: number;
  /** Секция-обёртка уже рисует свой заголовок (станции «Эфира»). */
  hideHeader?: boolean;
}

export const ClusterRow = React.memo(function ClusterRow({
  title,
  description,
  icon,
  index,
  tracks,
  queue,
  clusterId,
  cardWidth = 168,
  hideHeader = false,
}: Props) {
  const ctx = useMemo(() => ({ clusterId: String(clusterId) }), [clusterId]);
  return (
    <ClusterFeedbackProvider value={ctx}>
      <div className="flex flex-col gap-3.5">
        {!hideHeader && (
          <ClusterHeader icon={icon} title={title} description={description} index={index} />
        )}
        <HorizontalScroll>
          {tracks.map((track) => (
            <div key={track.urn} className="shrink-0" style={{ width: cardWidth }}>
              <TrackCard track={track} queue={queue} />
            </div>
          ))}
        </HorizontalScroll>
      </div>
    </ClusterFeedbackProvider>
  );
});
