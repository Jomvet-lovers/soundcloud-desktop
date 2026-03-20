import { invoke } from '@tauri-apps/api/core';
import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';
import { getCurrentTime } from './audio';

let connected = false;

async function ensureConnected(): Promise<boolean> {
  if (connected) return true;
  try {
    connected = await invoke<boolean>('discord_connect');
    return connected;
  } catch {
    return false;
  }
}

function artworkToLarge(url: string | null): string | undefined {
  if (!url) return undefined;
  return url.replace(/-[^-./]+(\.[^.]+)$/, '-t500x500$1');
}

async function updatePresence(track: Track) {
  if (!(await ensureConnected())) return;

  try {
    await invoke('discord_set_activity', {
      track: {
        title: track.title,
        artist: track.user.username,
        artwork_url: artworkToLarge(track.artwork_url),
        track_url: track.user.permalink_url
          ? `${track.user.permalink_url}`.replace(/\?.*$/, '')
          : undefined,
        duration_secs: Math.round(track.duration / 1000),
        elapsed_secs: Math.round(getCurrentTime()),
      },
    });
  } catch (e) {
    console.warn('[Discord] Failed to set activity:', e);
    connected = false;
  }
}

async function clearPresence() {
  if (!connected) return;
  try {
    await invoke('discord_clear_activity');
  } catch {
    connected = false;
  }
}

let lastUrn: string | null = null;
let lastPlaying = false;

usePlayerStore.subscribe((state) => {
  const { currentTrack, isPlaying } = state;

  const trackChanged = currentTrack?.urn !== lastUrn;
  const playChanged = isPlaying !== lastPlaying;

  if (!currentTrack || !isPlaying) {
    if (lastPlaying || trackChanged) {
      clearPresence();
    }
    lastUrn = currentTrack?.urn ?? null;
    lastPlaying = false;
    return;
  }

  if (trackChanged || playChanged) {
    lastUrn = currentTrack.urn;
    lastPlaying = isPlaying;
    updatePresence(currentTrack);
  }
});
