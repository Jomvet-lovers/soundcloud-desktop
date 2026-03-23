import { useVirtualizer } from '@tanstack/react-virtual';
import React, { useEffect, useRef, useState } from 'react';

interface VirtualListProps<T> {
  items: T[];
  rowHeight: number;
  overscan?: number;
  className?: string;
  disabled?: boolean;
  getItemKey: (item: T, index: number) => string;
  renderItem: (item: T, index: number) => React.ReactNode;
}

export function VirtualList<T>({
  items,
  rowHeight,
  overscan = 6,
  className,
  disabled = false,
  getItemKey,
  renderItem,
}: VirtualListProps<T>) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [scrollElement, setScrollElement] = useState<HTMLElement | null>(null);

  useEffect(() => {
    setScrollElement((containerRef.current?.closest('main') as HTMLElement | null) ?? null);
  }, []);

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollElement,
    estimateSize: () => rowHeight,
    overscan,
  });

  if (disabled) {
    return (
      <div ref={containerRef} className={className}>
        {items.map((item, index) => (
          <React.Fragment key={getItemKey(item, index)}>{renderItem(item, index)}</React.Fragment>
        ))}
      </div>
    );
  }

  const virtualItems = virtualizer.getVirtualItems();
  const totalHeight = virtualizer.getTotalSize();

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
