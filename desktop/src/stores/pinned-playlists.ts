import { useAuthStore } from './auth';
import { type SidebarPinnedPlaylist, useSettingsStore } from './settings';

/**
 * Sidebar pins are scoped to the signed-in SoundCloud account: switching
 * accounts swaps the Quick Access list, and the signed-out shell uses `anon`.
 * Kept outside the settings store so that store never has to import auth
 * (settings ← streaming ← api ← auth would be a cycle).
 */

const EMPTY: SidebarPinnedPlaylist[] = [];

function ownerKey(): string {
  return String(useAuthStore.getState().activeId ?? 'anon');
}

/** Pins of the active account (empty array when it has none). */
export function usePinnedPlaylists(): SidebarPinnedPlaylist[] {
  const activeId = useAuthStore((s) => s.activeId);
  const byAccount = useSettingsStore((s) => s.pinnedPlaylistsByAccount);
  return byAccount[String(activeId ?? 'anon')] ?? EMPTY;
}

export function pinPlaylist(playlist: SidebarPinnedPlaylist): void {
  useSettingsStore.getState().pinPlaylist(playlist, ownerKey());
}

export function unpinPlaylist(urn: string): void {
  useSettingsStore.getState().unpinPlaylist(urn, ownerKey());
}

/** Move the pre-multi-account pin list into the active account, once. */
export function claimLegacyPins(): void {
  useSettingsStore.getState().claimLegacyPins(ownerKey());
}
