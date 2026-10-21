import { useVirtualizer } from '@tanstack/react-virtual';
import React, { useLayoutEffect, useRef, useState } from 'react';
import { prefetchVisibleTracks } from '../../lib/audio';

interface VirtualListProps<T> {
  items: T[];
  rowHeight: number;
  overscan?: number;
  className?: string;
  disabled?: boolean;
  getItemKey: (item: T, index: number) => string;
  renderItem: (item: T, index: number) => React.ReactNode;
  /** URN of a row, enabling prefetch of the visible range (track lists only). */
  prefetchUrn?: (item: T) => string | null;
}

export function VirtualList<T>({
  items,
  rowHeight,
  overscan = 6,
  className,
  disabled = false,
  getItemKey,
  renderItem,
  prefetchUrn,
}: VirtualListProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);

  // Resolve the scroll element before paint to avoid a visible 0-row first frame.
  useLayoutEffect(() => {
    setScrollElement((containerRef.current?.closest('main') as HTMLElement | null) ?? null);
  }, []);

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollElement,
    estimateSize: () => rowHeight,
    overscan,
  });

  const virtualItems = virtualizer.getVirtualItems();
  const totalHeight = virtualizer.getTotalSize();
  // A primitive range key keeps the effect from re-running on every render just
  // because the virtualizer returns a fresh array.
  const visibleRange =
    !disabled && prefetchUrn && virtualItems.length > 0
      ? `${virtualItems[0].index}:${virtualItems[virtualItems.length - 1].index}`
      : '';

  useLayoutEffect(() => {
    if (!visibleRange) return;
    const [first, last] = visibleRange.split(':').map(Number);
    const urns: string[] = [];
    for (let i = first; i <= last && i < items.length; i++) {
      const urn = prefetchUrn?.(items[i]);
      if (urn) urns.push(urn);
    }
    prefetchVisibleTracks(urns);
  }, [visibleRange, items, prefetchUrn]);

  if (disabled) {
    return (
      <div ref={containerRef} className={className}>
        {items.map((item, index) => (
          <React.Fragment key={getItemKey(item, index)}>{renderItem(item, index)}</React.Fragment>
        ))}
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={className}
      style={{ height: totalHeight, position: 'relative', width: '100%' }}
    >
      {virtualItems.map((virtualItem) => {
        const item = items[virtualItem.index];
        return (
          <div
            key={getItemKey(item, virtualItem.index)}
            data-index={virtualItem.index}
            style={{
              position: 'absolute',
              top: 0,
              left: 0,
              width: '100%',
              height: virtualItem.size,
              transform: `translateY(${virtualItem.start}px)`,
            }}
          >
            {renderItem(item, virtualItem.index)}
          </div>
        );
      })}
    </div>
  );
}
