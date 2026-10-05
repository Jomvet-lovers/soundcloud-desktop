import type { Track } from '../stores/player';
import { usePlayerStore } from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import { getCurrentTime, subscribe as subscribeAudioTime } from './audio';
import { trackedInvoke as invoke } from './diagnostics';
import { getArtistDisplay, getDisplayTitle } from './track-display';

let connected = false;
let lastConnectAttemptAt = 0;
const CONNECT_RETRY_MS = 5000;

// Discord only carries the activity while audio is actually playing, so its
// progress clock never runs during a pause. Operations are serialized because
// a late "clear" landing after a newer "set" would hide a playing track.
let opChain: Promise<void> = Promise.resolve();

function enqueue(op: () => Promise<void>) {
  opChain = opChain.then(op, op);
}

async function ensureConnected(): Promise<boolean> {
  if (!useSettingsStore.getState().discordRpcEnabled) {
    return false;
  }
  if (connected) return true;
  const now = Date.now();
  if (now - lastConnectAttemptAt < CONNECT_RETRY_MS) {
    return false;
  }
  lastConnectAttemptAt = now;
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

function stripQuery(url: string | null | undefined): string | undefined {
  return url ? `${url}`.replace(/\?.*$/, '') : undefined;
}

async function setActivity(track: Track) {
  if (!(await ensureConnected())) return;

  try {
    const isPlaying = usePlayerStore.getState().isPlaying;
    const { discordRpcMode, discordRpcShowButton } = useSettingsStore.getState();
    const display = getArtistDisplay(track);
    await invoke('discord_set_activity', {
      track: {
        title: getDisplayTitle(track),
        artist: display.primary || track.user.username,
        artwork_url: artworkToLarge(track.artwork_url),
        track_url: stripQuery(track.permalink_url),
        artist_url: stripQuery(track.user.permalink_url),
        duration_secs: Math.round(track.duration / 1000),
        elapsed_secs: Math.round(getCurrentTime()),
        is_playing: isPlaying,
        mode: discordRpcMode,
        show_button: discordRpcShowButton,
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
let lastElapsed = 0;
let seekSyncTimer: ReturnType<typeof setTimeout> | null = null;

/** Reconcile Discord with the player: playing → activity, otherwise hidden. */
function syncPresence() {
  enqueue(async () => {
    const { currentTrack, isPlaying } = usePlayerStore.getState();
    if (currentTrack && isPlaying && useSettingsStore.getState().discordRpcEnabled) {
      await setActivity(currentTrack);
    } else {
      await clearPresence();
    }
  });
}

function schedulePresenceSync(delayMs: number) {
  if (seekSyncTimer) clearTimeout(seekSyncTimer);
  seekSyncTimer = setTimeout(() => {
    seekSyncTimer = null;
    syncPresence();
  }, delayMs);
}

usePlayerStore.subscribe((state) => {
  const { currentTrack, isPlaying } = state;

  const trackChanged = currentTrack?.urn !== lastUrn;
  const playChanged = isPlaying !== lastPlaying;
  if (!trackChanged && !playChanged) return;

  if (seekSyncTimer) {
    clearTimeout(seekSyncTimer);
    seekSyncTimer = null;
  }

  lastUrn = currentTrack?.urn ?? null;
  lastPlaying = isPlaying;
  lastElapsed = currentTrack ? Math.round(getCurrentTime()) : 0;

  syncPresence();
});

useSettingsStore.subscribe((state, prev) => {
  const rpcSettingsChanged =
    state.discordRpcEnabled !== prev.discordRpcEnabled ||
    state.discordRpcMode !== prev.discordRpcMode ||
    state.discordRpcShowButton !== prev.discordRpcShowButton;

  if (!rpcSettingsChanged) return;

  if (!state.discordRpcEnabled) {
    if (seekSyncTimer) {
      clearTimeout(seekSyncTimer);
      seekSyncTimer = null;
    }
    enqueue(async () => {
      await clearPresence();
      connected = false;
      await invoke('discord_disconnect').catch(() => undefined);
    });
    return;
  }

  syncPresence();
});

subscribeAudioTime(() => {
  const { currentTrack, isPlaying } = usePlayerStore.getState();
  if (!currentTrack || !isPlaying || !useSettingsStore.getState().discordRpcEnabled) return;

  if (!connected) {
    syncPresence();
    return;
  }

  const elapsed = Math.round(getCurrentTime());
  const drift = Math.abs(elapsed - lastElapsed);

  // Re-sync Discord timestamps on manual seek / large jumps without spamming updates every second.
  if (drift >= 2) {
    lastElapsed = elapsed;
    schedulePresenceSync(180);
  } else {
    lastElapsed = elapsed;
  }
});
