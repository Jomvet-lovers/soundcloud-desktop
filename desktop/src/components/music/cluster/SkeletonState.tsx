import React from 'react';
import { Skeleton } from '../../ui/Skeleton';

interface Props {
  rows?: number;
  itemsPerRow?: number;
  cardWidth?: number;
}

export const SkeletonState = React.memo(function SkeletonState({
  rows = 3,
  itemsPerRow = 6,
  cardWidth = 160,
}: Props) {
  return (
    <div className="flex flex-col gap-6">
      {Array.from({ length: rows }).map((_, row) => (
        <div key={row} className="flex flex-col gap-3">
          <div className="flex items-center gap-3 pl-1">
            <Skeleton className="h-6 w-6" rounded="lg" />
            <Skeleton className="h-4 w-32" rounded="sm" />
          </div>
          <div className="flex gap-3 overflow-hidden">
            {Array.from({ length: itemsPerRow }).map((_, i) => (
              <div key={i} className="shrink-0" style={{ width: cardWidth }}>
                <Skeleton className="aspect-square w-full" rounded="lg" />
                <Skeleton className="h-3 w-3/4 mt-2.5" rounded="sm" />
                <Skeleton className="h-2.5 w-1/2 mt-1.5" rounded="sm" />
              </div>
            ))}
          </div>
        </div>
      ))}
    </div>
  );
});
