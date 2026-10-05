import React from 'react';
import { useShallow } from 'zustand/shallow';
import { ListMusic, Trash2, X } from '../../lib/icons';
import { usePlayerStore } from '../../stores/player';
import { NowPlayingCard } from './queue/NowPlayingCard';
import { QueueList } from './queue/QueueList';

/* ── Queue drawer ─────────────────────────────────────────────
 * Right-side flat drawer: solid surface, content isolated on top. */

export const QueuePanel = React.memo(
  ({ open, onClose }: { open: boolean; onClose: () => void }) => {
    const { currentTrack, queueLength, queueIndex, isPlaying } = usePlayerStore(
      useShallow((s) => ({
        currentTrack: s.currentTrack,
        queueLength: s.queue.length,
        queueIndex: s.queueIndex,
        isPlaying: s.isPlaying,
      })),
    );

    const upNextCount = queueLength - queueIndex - 1;

    return (
      <>
        {/* Backdrop */}
        <div
          className={`fixed inset-0 bg-black/60 z-40 transition-opacity duration-300 ${
            open ? 'opacity-100' : 'opacity-0 pointer-events-none'
          }`}
          onClick={onClose}
        />

        {/* Panel */}
        <div
          className="fixed top-0 right-0 bottom-0 w-[360px] z-50 flex flex-col border-l border-white/[0.06]"
          style={{
            transform: open ? 'translateX(0)' : 'translateX(100%)',
            visibility: open ? 'visible' : 'hidden',
            transition: 'transform 300ms cubic-bezier(0.16, 1, 0.3, 1), visibility 300ms',
          }}
        >
          {/* Flat surface (solid). */}
          <div
            className="absolute inset-0 overflow-hidden"
            style={{ contain: 'strict', transform: 'translateZ(0)' }}
          >
            <div className="absolute inset-0" style={{ background: '#101014' }} />
          </div>
          {/* accent edge */}
          <div
            className="absolute inset-y-0 left-0 w-px pointer-events-none"
            style={{
              background: 'var(--color-accent)',
              opacity: 0.5,
            }}
          />

          {/* Content */}
          <div className="relative z-10 flex flex-col h-full" style={{ isolation: 'isolate' }}>
            {/* Header */}
            <div
              className="flex items-center justify-between px-5 pt-5 pb-3"
              data-tauri-drag-region
            >
              <div className="flex items-center gap-2.5">
                <h2 className="text-[15px] font-semibold tracking-tight text-white/90">
                  {'Queue'}
                </h2>
                {queueLength > 0 && (
                  <span className="text-[11px] font-semibold text-white/40 bg-white/[0.06] rounded-full px-2 py-0.5 tabular-nums">
                    {queueLength}
                  </span>
                )}
              </div>
              <div className="flex items-center gap-1">
                {queueLength > 0 && (
                  <button
                    type="button"
                    onClick={() => usePlayerStore.getState().clearQueue()}
                    className="h-7 px-2.5 rounded-lg text-[11px] text-white/30 hover:text-white/60 hover:bg-white/[0.06] transition-all duration-150 cursor-pointer flex items-center gap-1.5"
                  >
                    <Trash2 size={12} />
                    {'Clear Queue'}
                  </button>
                )}
                <button
                  type="button"
                  onClick={onClose}
                  title={'Close'}
                  className="w-7 h-7 rounded-lg flex items-center justify-center text-white/30 hover:text-white/60 hover:bg-white/[0.06] transition-all duration-150 cursor-pointer"
                >
                  <X size={16} />
                </button>
              </div>
            </div>

            {/* Now Playing */}
            {currentTrack && (
              <div className="px-3.5 pb-2">
                <p className="text-[11px] text-white/40 font-medium mb-2 px-1.5">{'Now Playing'}</p>
                <NowPlayingCard />
              </div>
            )}

            {/* Up Next */}
            <div className="flex-1 overflow-y-auto scrollbar-hide px-3.5 pb-4">
              {upNextCount > 0 && (
                <>
                  <p className="text-[11px] text-white/40 font-medium mb-2 mt-3 px-1.5">
                    {'Up Next'} · {upNextCount}
                  </p>
                  <QueueList
                    startIndex={queueIndex + 1}
                    queueIndex={queueIndex}
                    isPlaying={isPlaying}
                  />
                </>
              )}

              {queueLength === 0 && (
                <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-8">
                  <div className="w-14 h-14 rounded-2xl bg-white/[0.04] ring-1 ring-white/[0.06] flex items-center justify-center">
                    <ListMusic size={24} className="text-white/15" />
                  </div>
                  <div>
                    <p className="text-[14px] text-white/40 font-medium">{'Queue is empty'}</p>
                    <p className="text-[12px] text-white/20 mt-1 leading-relaxed max-w-[200px]">
                      {'Tracks you play next will show up here'}
                    </p>
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </>
    );
  },
);
