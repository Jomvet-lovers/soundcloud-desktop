import React, { useCallback, useState } from 'react';
import { Outlet } from 'react-router-dom';
import { art } from '../../lib/cdn';
import { usePlayerStore } from '../../stores/player';
import { QueuePanel } from '../music/QueuePanel';
import { NowPlayingBar } from './NowPlayingBar';
import { Sidebar } from './Sidebar';
import { Titlebar } from './Titlebar';

/* Ambient glow — isolated, doesn't re-render AppShell */
const AmbientGlow = React.memo(() => {
  const artwork = usePlayerStore((s) => art(s.currentTrack?.artwork_url, 't500x500'));
  if (!artwork) return null;
  return (
    <div
      className="absolute bottom-0 left-0 right-0 h-[400px] opacity-[0.06] blur-[100px] pointer-events-none transition-all duration-[2s] ease-out"
      style={{
        backgroundImage: `url(${artwork})`,
        backgroundSize: 'cover',
        backgroundPosition: 'center',
      }}
    />
  );
});

/* Memoized Outlet — won't re-render when AppShell state changes.
   Route changes propagate via React Router context, bypassing memo. */
const StableOutlet = React.memo(() => <Outlet />);

export const AppShell = React.memo(() => {
  const [queueOpen, setQueueOpen] = useState(false);
  const onQueueToggle = useCallback(() => setQueueOpen((v) => !v), []);
  const onQueueClose = useCallback(() => setQueueOpen(false), []);

  return (
    <div className="flex flex-col h-screen relative overflow-hidden">
      <AmbientGlow />
      <Titlebar />
      <div className="flex flex-1 min-h-0 relative z-10">
        <Sidebar />
        <main className="flex-1 overflow-y-auto overflow-x-hidden">
          <StableOutlet />
        </main>
      </div>
      <NowPlayingBar onQueueToggle={onQueueToggle} queueOpen={queueOpen} />
      <QueuePanel open={queueOpen} onClose={onQueueClose} />
    </div>
  );
});
