import React, { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { fc } from '../../lib/formatters';
import { type Aura, auraRgba } from '../../lib/aura';

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

function TabDockImpl<T extends string>({ tabs, active, onChange, aura }: TabDockProps<T>) {
  const dockRef = useRef<HTMLDivElement>(null);
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);

  useIsoLayoutEffect(() => {
    const dock = dockRef.current;
    if (!dock) return;
    const btn = dock.querySelector<HTMLButtonElement>(`[data-tab="${active}"]`);
    if (!btn) return;

    const update = () => {
      const dockRect = dock.getBoundingClientRect();
      const r = btn.getBoundingClientRect();
      setPill({ x: r.left - dockRect.left, w: r.width });
    };

    update();
    const ro = new ResizeObserver(update);
    ro.observe(dock);
    ro.observe(btn);
    return () => ro.disconnect();
  }, [active, tabs]);

  return (
    <div className="sticky top-3 z-40 flex justify-center pointer-events-none">
      <div
        ref={dockRef}
        className="pointer-events-auto relative flex items-center gap-1 p-1.5 rounded-2xl"
        style={{
          background: 'rgba(15,15,18,0.55)',
          backdropFilter: 'blur(40px) saturate(180%)',
          WebkitBackdropFilter: 'blur(40px) saturate(180%)',
          boxShadow:
            '0 24px 60px rgba(0,0,0,0.45), inset 0 0 0 1px rgba(255,255,255,0.08), inset 0 1px 0 rgba(255,255,255,0.08)',
        }}
      >
        {pill && (
          <div
            className="absolute top-1.5 bottom-1.5 rounded-xl transition-all duration-500 ease-[cubic-bezier(0.2,0.8,0.2,1)]"
            style={{
              left: pill.x,
              width: pill.w,
              background: `linear-gradient(180deg, ${auraRgba(aura, 0.22)}, ${auraRgba(aura, 0.06)})`,
              border: `0.5px solid ${auraRgba(aura, 0.35)}`,
              boxShadow: `0 6px 20px ${auraRgba(aura, 0.25)}, inset 0 0.5px 0 rgba(255,255,255,0.12)`,
            }}
          />
        )}
        {tabs.map((tab) => {
          const isActive = active === tab.id;
          return (
            <button
              key={tab.id}
              type="button"
              data-tab={tab.id}
              onClick={() => onChange(tab.id)}
              className={`relative z-10 inline-flex items-center gap-2 px-4 h-9 rounded-xl text-[12.5px] font-semibold transition-colors duration-300 cursor-pointer ${
                isActive ? 'text-white' : 'text-white/45 hover:text-white/85'
              }`}
            >
              {tab.label}
              {tab.count != null && (
                <span
                  className="text-[10px] tabular-nums font-bold px-1.5 py-0.5 rounded-md transition-colors"
                  style={{
                    background: isActive ? 'rgba(255,255,255,0.16)' : 'rgba(255,255,255,0.05)',
                    color: isActive ? '#fff' : 'rgba(255,255,255,0.35)',
                  }}
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
