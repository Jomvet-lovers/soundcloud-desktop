import React, { useState } from 'react';
import { Outlet } from 'react-router-dom';
import { art } from '../../lib/cdn';
import { useCdnUrl } from '../../lib/useCdnUrl';
import { usePlayerStore } from '../../stores/player';
import { QueuePanel } from '../music/QueuePanel';
import { NowPlayingBar } from './NowPlayingBar';
import { Sidebar } from './Sidebar';
import { Titlebar } from './Titlebar';

export const AppShell = React.memo(() => {
  const [queueOpen, setQueueOpen] = useState(false);
  const rawArtwork = usePlayerStore((s) => art(s.currentTrack?.artwork_url, 't500x500'));
  const artwork = useCdnUrl(rawArtwork);

  return (
    <div className="flex flex-col h-screen relative overflow-hidden">
      {/* Ambient background glow from current track */}
      {artwork && (
        <div
          className="absolute bottom-0 left-0 right-0 h-[400px] opacity-[0.06] blur-[100px] pointer-events-none transition-all duration-[2s] ease-out"
          style={{
            backgroundImage: `url(${artwork})`,
            backgroundSize: 'cover',
            backgroundPosition: 'center',
          }}
        />
      )}

      <Titlebar />
      <div className="flex flex-1 min-h-0 relative">
        <Sidebar />
        <main className="flex-1 overflow-y-auto overflow-x-hidden">
          <Outlet />
        </main>
      </div>
      <NowPlayingBar onQueueToggle={() => setQueueOpen((v) => !v)} queueOpen={queueOpen} />
      <QueuePanel open={queueOpen} onClose={() => setQueueOpen(false)} />
    </div>
  );
});
