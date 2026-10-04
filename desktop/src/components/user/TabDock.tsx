import React, { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { Aura } from '../../lib/aura';
import { fc } from '../../lib/formatters';

export type TabId = 'popular' | 'tracks' | 'playlists' | 'likes' | 'followers' | 'following';

export interface TabDescriptor<T extends string = string> {
  id: T;
  label: string;
  count?: number | null;
}

interface TabDockProps<T extends string = string> {
  tabs: ReadonlyArray<TabDescriptor<T>>;
  active: T;
  onChange: (id: T) => void;
  aura: Aura;
}

const useIsoLayoutEffect = typeof window !== 'undefined' ? useLayoutEffect : useEffect;

/** Flat, scrollable tab row — no container card, active tab is a subtle chip. */
function TabDockImpl<T extends string>({ tabs, active, onChange }: TabDockProps<T>) {
  const dockRef = useRef<HTMLDivElement>(null);
  const [overflows, setOverflows] = useState(false);
  const dragRef = useRef({ active: false, startX: 0, startScroll: 0, moved: false });

  useIsoLayoutEffect(() => {
    const dock = dockRef.current;
    if (!dock) return;
    const update = () => setOverflows(dock.scrollWidth > dock.clientWidth + 1);
    update();
    const ro = new ResizeObserver(update);
    ro.observe(dock);
    return () => ro.disconnect();
  }, [tabs]);

  const onPointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    const dock = dockRef.current;
    if (!dock || dock.scrollWidth <= dock.clientWidth + 1) return;
    dragRef.current = {
      active: true,
      startX: e.clientX,
      startScroll: dock.scrollLeft,
      moved: false,
    };
  }, []);

  const onPointerMove = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag.active) return;
    const dock = dockRef.current;
    if (!dock) return;
    const dx = e.clientX - drag.startX;
    if (!drag.moved && Math.abs(dx) > 5) {
      drag.moved = true;
      dock.setPointerCapture(e.pointerId);
      dock.style.cursor = 'grabbing';
    }
    if (drag.moved) {
      dock.scrollLeft = drag.startScroll - dx;
    }
  }, []);

  const endDrag = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current;
    if (!drag.active) return;
    drag.active = false;
    drag.moved = false;
    const dock = dockRef.current;
    if (!dock) return;
    if (dock.hasPointerCapture(e.pointerId)) dock.releasePointerCapture(e.pointerId);
    dock.style.cursor = '';
  }, []);

  const onClickCapture = useCallback((e: React.MouseEvent<HTMLDivElement>) => {
    if (dragRef.current.moved) {
      e.preventDefault();
      e.stopPropagation();
    }
  }, []);

  const onWheel = useCallback((e: React.WheelEvent<HTMLDivElement>) => {
    const dock = dockRef.current;
    if (!dock) return;
    if (Math.abs(e.deltaX) > Math.abs(e.deltaY)) return;
    dock.scrollLeft += e.deltaY;
  }, []);

  const [isHovered, setIsHovered] = useState(false);

  useEffect(() => {
    const dock = dockRef.current;
    if (!dock) return;

    const onKeyDown = (e: KeyboardEvent) => {
      if (!isHovered) return;
      if (dock.scrollWidth <= dock.clientWidth + 1) return;

      if (e.key === 'PageUp') {
        dock.scrollBy({ left: -dock.clientWidth * 0.8, behavior: 'smooth' });
        e.preventDefault();
      } else if (e.key === 'PageDown') {
        dock.scrollBy({ left: dock.clientWidth * 0.8, behavior: 'smooth' });
        e.preventDefault();
      }
    };

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [isHovered]);

  return (
    <div className="sticky top-3 z-40 flex justify-center pointer-events-none px-2 sm:px-4">
      <div
        ref={dockRef}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onClickCapture={onClickCapture}
        onWheel={onWheel}
        onMouseEnter={() => setIsHovered(true)}
        onMouseLeave={() => setIsHovered(false)}
        className={`pointer-events-auto relative flex items-center gap-1 min-w-0 max-w-full overflow-x-auto overscroll-x-contain touch-pan-x select-none [&::-webkit-scrollbar]:hidden [scrollbar-width:none] ${
          overflows ? 'cursor-grab' : 'cursor-default'
        }`}
      >
        {tabs.map((tab) => {
          const isActive = active === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              data-tab={tab.id}
              onClick={() => onChange(tab.id)}
              className={`shrink-0 inline-flex items-center gap-1.5 px-2.5 sm:px-3 h-8 rounded-md text-[12px] sm:text-[12.5px] font-medium transition-colors ${
                overflows ? 'cursor-grab' : 'cursor-pointer'
              } ${isActive ? 'bg-white/[0.08] text-white' : 'text-white/45 hover:text-white/80'}`}
            >
              <span className="whitespace-nowrap">{tab.label}</span>
              {tab.count != null && (
                <span
                  className={`hidden sm:inline-flex text-[10px] tabular-nums font-medium px-1.5 py-0.5 rounded-md ${
                    isActive ? 'text-white/80' : 'text-white/30'
                  }`}
                >
                  {fc(tab.count)}
                </span>
              )}
            </button>
          );
        })}
      </div>
    </div>
  );
}

export const TabDock = React.memo(TabDockImpl) as typeof TabDockImpl;
