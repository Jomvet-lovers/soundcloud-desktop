import { createElement, useEffect, type ReactNode } from 'react';
import { View } from 'react-native';
import type { Me } from '@sc/data';
import { artUrl, Atmosphere, TooltipHost, useScTheme } from '@sc/ui';
import { I18nProvider } from '../i18n';
import { EqProvider } from '../player/EqContext';
import { useAmbient } from '../player/ambient';
import { usePlayerState } from '../player/PlayerContext';
import { getPositionSecs } from '../player/position';
import type { Router } from '../nav/router';
import { AddToPlaylistDialog } from './AddToPlaylistDialog';
import { Titlebar } from './Titlebar';
import { Sidebar } from './Sidebar';
import { LyricsScreen } from './LyricsScreen';
import { NowPlayingBar } from './NowPlayingBar';
import { PanelsProvider } from './panels';
import { PlayerPanels } from './PlayerPanels';

/** Блюр-блум обложки текущего трека внизу экрана (донор AmbientGlow). */
function AmbientGlow({ artworkUrl }: { artworkUrl: string | null | undefined }) {
  const { perf } = useScTheme();
  const uri = artUrl(artworkUrl, 't500x500');
  if (!perf.bloom || !uri) return null;
  return createElement('img', {
    src: uri,
    style: {
      position: 'absolute',
      bottom: 0,
      left: '10%',
      width: '80%',
      height: 400,
      objectFit: 'cover',
      filter: 'blur(100px)',
      opacity: 0.06,
      pointerEvents: 'none',
    },
  });
}

function isTypingTarget(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === 'INPUT' || tag === 'TEXTAREA' || el.isContentEditable;
}

/** Каркас приложения (донор AppShell.tsx): Titlebar + Sidebar + контент + NowPlayingBar. */
export function AppShell({ router, me, children }: { router: Router; me: Me | null; children: ReactNode }) {
  const player = usePlayerState();
  const ambient = useAmbient();

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (isTypingTarget(e.target)) return;
      switch (e.code) {
        case 'Space':
          e.preventDefault();
          player.togglePlayPause();
          break;
        case 'ArrowRight':
          if (player.currentTrack) player.seek(Math.min(player.currentTrack.duration_ms / 1000, getPositionSecs() + 5));
          break;
        case 'ArrowLeft':
          player.seek(Math.max(0, getPositionSecs() - 5));
          break;
        case 'ArrowUp':
          e.preventDefault();
          player.setVolume(player.volume + 5);
          break;
        case 'ArrowDown':
          e.preventDefault();
          player.setVolume(player.volume - 5);
          break;
        case 'KeyN':
          player.next();
          break;
        case 'KeyP':
          player.prev();
          break;
        case 'KeyS':
          player.toggleShuffle();
          break;
        case 'KeyR':
          player.cycleRepeat();
          break;
      }
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [player]);

  return (
    <I18nProvider>
      <EqProvider>
        <PanelsProvider>
          <View style={{ flex: 1, height: '100%' as never, overflow: 'hidden', backgroundColor: '#08080a' }}>
            {ambient && <Atmosphere tint={ambient.colors} energy={ambient.energy} />}
            <View style={{ flex: 1, minHeight: 0, zIndex: 1 }}>
              <AmbientGlow artworkUrl={player.currentTrack?.artwork_url} />
              <Titlebar router={router} />
              <View style={{ flex: 1, flexDirection: 'row', minHeight: 0 }}>
                <Sidebar route={router.route} navigate={router.navigate} me={me} />
                <View style={{ flex: 1, minWidth: 0 }}>{children}</View>
              </View>
              <PlayerPanels />
              <NowPlayingBar />
              <LyricsScreen />
              <AddToPlaylistDialog />
              <TooltipHost />
            </View>
          </View>
        </PanelsProvider>
      </EqProvider>
    </I18nProvider>
  );
}
