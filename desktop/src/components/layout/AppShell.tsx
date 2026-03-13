import React, { useCallback, useState } from 'react';
import { Outlet } from 'react-router-dom';
import { getWallpaperUrl } from '../../lib/cache';
import { art } from '../../lib/formatters';
import { usePlayerStore } from '../../stores/player';
import { useSettingsStore } from '../../stores/settings';
import { LyricsPanel } from '../music/LyricsPanel';
import { QueuePanel } from '../music/QueuePanel';
import { NowPlayingBar } from './NowPlayingBar';
import { Sidebar } from './Sidebar';
import { Titlebar } from './Titlebar';

/* Custom background image — from settings */
const CustomBackground = React.memo(() => {
  const bgName = useSettingsStore((s) => s.backgroundImage);
  const bgOpacity = useSettingsStore((s) => s.backgroundOpacity);

  const bgUrl = bgName ? getWallpaperUrl(bgName) : null;
  if (!bgUrl) return null;
  return (
    <div
      className="absolute inset-0 pointer-events-none bg-cover bg-center bg-no-repeat transition-opacity duration-1000"
      style={{
        backgroundImage: `url(${bgUrl})`,
        opacity: bgOpacity,
      }}
    />
  );
});

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
      <CustomBackground />
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
      <LyricsPanel />
    </div>
  );
});
