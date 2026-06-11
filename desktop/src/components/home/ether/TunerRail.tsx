import React, {useEffect, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {usePerfMode} from '../../../lib/perf';
import {usePlayerStore} from '../../../stores/player';

export interface StationDef {
  id: string;
  /** Фейковая частота диапазона — тех-метка станции. */
  freq: number;
  title: string;
}

const BAND_START = 7;
const BAND_SPAN = 86;

export function stationPos(index: number, count: number): number {
  if (count <= 1) return BAND_START + BAND_SPAN / 2;
  return BAND_START + (index / (count - 1)) * BAND_SPAN;
}

const TICKS = Array.from({ length: 41 }, (_, i) => i);

function Clock() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(timer);
  }, []);
  const hh = String(now.getHours()).padStart(2, '0');
  const mm = String(now.getMinutes()).padStart(2, '0');
  return (
    <span className="font-mono text-[14px] text-white/55 tabular-nums">
      {hh}:{mm}
    </span>
  );
}

/** Sticky шкала-тюнер: хребет страницы. Станции = секции на диапазоне,
 *  игла защёлкивается на активную CSS-транзишеном, клик = перемотка эфира. */
export const TunerRail = React.memo(function TunerRail({
  stations,
  activeId,
  onSelect,
}: {
  stations: StationDef[];
  activeId: string | null;
  onSelect: (id: string) => void;
}) {
  const { t } = useTranslation();
  const perf = usePerfMode();
  const isPlaying = usePlayerStore((s) => s.isPlaying);
  const b = perf.blur(20);
  const activeIndex = Math.max(
    0,
    stations.findIndex((s) => s.id === activeId),
  );
  const activeStation = stations[activeIndex];
  const needleLeft = stationPos(activeIndex, stations.length);

  return (
    <div
      className="sticky top-0 z-30 -mx-1 rounded-b-[18px] border-b border-white/[0.07] px-1"
      style={{
        background: b > 0 ? 'rgba(10,10,12,0.74)' : 'rgb(12,12,15)',
        backdropFilter: b > 0 ? `blur(${b}px) saturate(1.3)` : undefined,
        WebkitBackdropFilter: b > 0 ? `blur(${b}px) saturate(1.3)` : undefined,
      }}
    >
      <div className="flex h-[84px] items-center gap-6 px-4">
        <div className="w-[112px] flex-none">
          <div className="font-mono text-[27px] font-medium leading-none tracking-[-0.01em] text-white/92 tabular-nums">
            {activeStation ? activeStation.freq.toFixed(1) : '—'}
          </div>
          <div className="mt-1.5 flex items-center gap-2">
            <span
              className="eth-anim size-1.5 rounded-full"
              style={{
                background: 'var(--color-accent)',
                boxShadow: perf.glow ? '0 0 8px var(--color-accent)' : undefined,
                animation:
                  isPlaying && perf.idleAnim ? 'eth-pulse 1.6s ease-in-out infinite' : undefined,
              }}
            />
            <span className="text-[9.5px] font-semibold uppercase tracking-[0.18em] text-white/35">
              {t('soundwave.ether.onAir')}
            </span>
          </div>
        </div>

        <div className="relative h-[62px] min-w-0 flex-1">
          <div className="absolute bottom-3 left-0 right-0 flex justify-between">
            {TICKS.map((i) => (
              <span
                key={i}
                className={
                  i % 5 === 0 ? 'h-[15px] w-px bg-white/[0.16]' : 'h-[8px] w-px bg-white/[0.08]'
                }
                style={{ alignSelf: 'flex-end' }}
              />
            ))}
          </div>
          <span className="absolute -bottom-0.5 left-0 text-[8.5px] font-semibold uppercase tracking-[0.2em] text-white/25">
            {t('soundwave.ether.bandCore')}
          </span>
          <span className="absolute -bottom-0.5 right-0 text-[8.5px] font-semibold uppercase tracking-[0.2em] text-white/25">
            {t('soundwave.ether.bandFrontier')}
          </span>

          {stations.map((station, i) => {
            const isActive = station.id === activeId;
            return (
              <button
                key={station.id}
                type="button"
                onClick={() => onSelect(station.id)}
                className="absolute top-1 flex -translate-x-1/2 cursor-pointer flex-col items-center gap-1.5"
                style={{ left: `${stationPos(i, stations.length)}%` }}
                title={station.title}
              >
                <span
                  className={`whitespace-nowrap text-[9.5px] font-semibold tracking-[0.1em] transition-colors ${
                    isActive ? 'text-white/90' : 'text-white/35 hover:text-white/60'
                  }`}
                >
                  {station.title}
                </span>
                <span
                  className="size-[5px] rounded-full transition-all"
                  style={
                    isActive
                      ? {
                          background: 'var(--color-accent)',
                          boxShadow: perf.glow ? '0 0 8px var(--color-accent)' : undefined,
                        }
                      : { background: 'rgba(255,255,255,0.3)' }
                  }
                />
                <span className="font-mono text-[8.5px] text-white/25 tabular-nums">
                  {station.freq.toFixed(1)}
                </span>
              </button>
            );
          })}

          <span
            className="pointer-events-none absolute bottom-2 top-3 w-[2px] rounded-[1px]"
            style={{
              left: `${needleLeft}%`,
              background: 'var(--color-accent)',
              boxShadow: perf.glow
                ? '0 0 10px var(--color-accent-glow), 0 0 26px var(--color-accent-glow)'
                : undefined,
              transition: 'left 420ms var(--ease-apple)',
            }}
          />
        </div>

        <div className="hidden w-[88px] flex-none flex-col items-end gap-1.5 md:flex">
          <Clock />
          <div className="flex h-3 items-end gap-[2.5px]">
            {[4, 7, 10, 12].map((h, i) => (
              <span
                key={h}
                className="w-[3px] rounded-[1px]"
                style={{
                  height: h,
                  background: i < 3 ? 'var(--color-accent)' : 'rgba(255,255,255,0.16)',
                }}
              />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
});
