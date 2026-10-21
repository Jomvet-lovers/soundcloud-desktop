import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import type { PerfMode } from '../lib/perf';
import { tauriStorage } from '../lib/tauri-storage';

export type ThemePreset = 'soundcloud' | 'dark' | 'neon' | 'forest' | 'crimson' | 'custom';
export type StartupPage = 'home' | 'feed' | 'search' | 'library' | 'settings';
export type DiscordRpcMode = 'track' | 'artist' | 'activity';
export type InputMode = 'auto' | 'touch';
export interface SidebarPinnedPlaylist {
  urn: string;
  title: string;
  artworkUrl: string | null;
}

export interface ThemePresetDef {
  accent: string;
  bg: string;
  /** [accent, bg, card] for preview swatch */
  preview: [string, string, string];
}

export const THEME_PRESETS: Record<Exclude<ThemePreset, 'custom'>, ThemePresetDef> = {
  soundcloud: {
    accent: '#ff5500',
    bg: '#08080a',
    preview: ['#ff5500', '#08080a', '#1a1a1e'],
  },
  dark: {
    accent: '#ffffff',
    bg: '#000000',
    preview: ['#ffffff', '#000000', '#111111'],
  },
  neon: {
    accent: '#bf5af2',
    bg: '#08060f',
    preview: ['#bf5af2', '#08060f', '#18102a'],
  },
  forest: {
    accent: '#22c55e',
    bg: '#050e08',
    preview: ['#22c55e', '#050e08', '#0a1f10'],
  },
  crimson: {
    accent: '#ff2d55',
    bg: '#0c0507',
    preview: ['#ff2d55', '#0c0507', '#1e0a10'],
  },
};

export interface SettingsState {
  accentColor: string;
  bgPrimary: string;
  themePreset: ThemePreset;
  perfMode: PerfMode;
  backgroundImage: string;
  backgroundOpacity: number;
  backgroundDim: number;
  backgroundBlur: number;
  glassBlur: number;
  audioCacheLimitMB: number;
  eqEnabled: boolean;
  eqGains: number[];
  eqPreset: string;
  normalizeVolume: boolean;
  highQualityStreaming: boolean;
  autoplayStation: boolean;
  sidebarCollapsed: boolean;
  floatingComments: boolean;
  startupPage: StartupPage;
  /** Sidebar pins per SoundCloud account id (`anon` when signed out). */
  pinnedPlaylistsByAccount: Record<string, SidebarPinnedPlaylist[]>;
  discordRpcEnabled: boolean;
  discordRpcMode: DiscordRpcMode;
  discordRpcShowButton: boolean;
  touchKeyboardAutoShow: boolean;
  inputMode: InputMode;
  /** Output device chosen manually (cpal device id on Win/mac, pactl sink name
   *  on Linux); null = follow the Windows default. Restored at startup. */
  audioOutputDevice: string | null;
  /** Whether the OS default device change is followed live. */
  followDefaultOutput: boolean;
  setAccentColor: (color: string) => void;
  setBgPrimary: (bg: string) => void;
  setThemePreset: (id: ThemePreset) => void;
  setPerfMode: (mode: PerfMode) => void;
  setBackgroundImage: (url: string) => void;
  setBackgroundOpacity: (opacity: number) => void;
  setBackgroundDim: (dim: number) => void;
  setBackgroundBlur: (blur: number) => void;
  setGlassBlur: (blur: number) => void;
  setAudioCacheLimitMB: (limit: number) => void;
  setEqEnabled: (enabled: boolean) => void;
  setEqGains: (gains: number[]) => void;
  setEqPreset: (preset: string) => void;
  setEqBand: (index: number, gain: number) => void;
  setNormalizeVolume: (enabled: boolean) => void;
  setHighQualityStreaming: (enabled: boolean) => void;
  setAutoplayStation: (enabled: boolean) => void;
  toggleSidebar: () => void;
  setFloatingComments: (v: boolean) => void;
  setStartupPage: (page: StartupPage) => void;
  pinPlaylist: (playlist: SidebarPinnedPlaylist, ownerKey: string) => void;
  unpinPlaylist: (urn: string, ownerKey: string) => void;
  /** Move the pre-multi-account pin list into its first signed-in account. */
  claimLegacyPins: (ownerKey: string) => void;
  setDiscordRpcEnabled: (enabled: boolean) => void;
  setDiscordRpcMode: (mode: DiscordRpcMode) => void;
  setDiscordRpcShowButton: (show: boolean) => void;
  setTouchKeyboardAutoShow: (enabled: boolean) => void;
  setInputMode: (mode: InputMode) => void;
  setAudioOutputDevice: (device: string | null) => void;
  setFollowDefaultOutput: (follow: boolean) => void;
  resetTheme: () => void;
}

const DEFAULT_EQ_GAINS = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

const DEFAULTS = {
  accentColor: '#ff5500',
  bgPrimary: '#08080a',
  themePreset: 'soundcloud' as ThemePreset,
  perfMode: 'beauty' as PerfMode,
  backgroundImage: '',
  backgroundOpacity: 0.15,
  backgroundDim: 0.6,
  backgroundBlur: 0,
  glassBlur: 40,
  audioCacheLimitMB: 1024,
  eqEnabled: false,
  eqGains: DEFAULT_EQ_GAINS,
  eqPreset: 'flat',
  normalizeVolume: false,
  highQualityStreaming: true,
  autoplayStation: true,
  sidebarCollapsed: false,
  floatingComments: true,
  startupPage: 'home' as StartupPage,
  pinnedPlaylistsByAccount: {} as Record<string, SidebarPinnedPlaylist[]>,
  discordRpcEnabled: true,
  discordRpcMode: 'track' as DiscordRpcMode,
  discordRpcShowButton: true,
  touchKeyboardAutoShow: false,
  inputMode: 'auto' as InputMode,
  audioOutputDevice: null,
  followDefaultOutput: true,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULTS,
      setAccentColor: (accentColor) => set({ accentColor, themePreset: 'custom' }),
      setBgPrimary: (bgPrimary) => set({ bgPrimary, themePreset: 'custom' }),
      setThemePreset: (id) => {
        if (id === 'custom') {
          set({ themePreset: 'custom' });
        } else {
          const preset = THEME_PRESETS[id];
          set({ themePreset: id, accentColor: preset.accent, bgPrimary: preset.bg });
        }
      },
      setPerfMode: (perfMode) => set({ perfMode }),
      setBackgroundImage: (backgroundImage) => set({ backgroundImage }),
      setBackgroundOpacity: (backgroundOpacity) => set({ backgroundOpacity }),
      setBackgroundDim: (backgroundDim) => set({ backgroundDim }),
      setBackgroundBlur: (backgroundBlur) => set({ backgroundBlur }),
      setGlassBlur: (glassBlur) => set({ glassBlur }),
      setAudioCacheLimitMB: (audioCacheLimitMB) => set({ audioCacheLimitMB }),
      setEqEnabled: (eqEnabled) => set({ eqEnabled }),
      setEqGains: (eqGains) => set({ eqGains, eqPreset: 'custom' }),
      setEqPreset: (eqPreset) => set({ eqPreset }),
      setEqBand: (index, gain) =>
        set((s) => {
          const eqGains = [...s.eqGains];
          eqGains[index] = gain;
          return { eqGains, eqPreset: 'custom' };
        }),
      setNormalizeVolume: (normalizeVolume) => set({ normalizeVolume }),
      setHighQualityStreaming: (highQualityStreaming) => set({ highQualityStreaming }),
      setAutoplayStation: (autoplayStation) => set({ autoplayStation }),
      toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
      setFloatingComments: (floatingComments) => set({ floatingComments }),
      setStartupPage: (startupPage) => set({ startupPage }),
      pinPlaylist: (playlist, ownerKey) =>
        set((s) => {
          const current = s.pinnedPlaylistsByAccount[ownerKey] ?? [];
          return {
            pinnedPlaylistsByAccount: {
              ...s.pinnedPlaylistsByAccount,
              [ownerKey]: [playlist, ...current.filter((item) => item.urn !== playlist.urn)].slice(
                0,
                8,
              ),
            },
          };
        }),
      unpinPlaylist: (urn, ownerKey) =>
        set((s) => ({
          pinnedPlaylistsByAccount: {
            ...s.pinnedPlaylistsByAccount,
            [ownerKey]: (s.pinnedPlaylistsByAccount[ownerKey] ?? []).filter(
              (item) => item.urn !== urn,
            ),
          },
        })),
      claimLegacyPins: (ownerKey) =>
        set((s) => {
          const next = { ...s.pinnedPlaylistsByAccount };
          const legacy = next.legacy;
          if (!legacy) return {};
          delete next.legacy;
          const target = next[ownerKey] ?? [];
          next[ownerKey] = target.length > 0 ? target : legacy;
          return { pinnedPlaylistsByAccount: next };
        }),
      setDiscordRpcEnabled: (discordRpcEnabled) => set({ discordRpcEnabled }),
      setDiscordRpcMode: (discordRpcMode) => set({ discordRpcMode }),
      setDiscordRpcShowButton: (discordRpcShowButton) => set({ discordRpcShowButton }),
      setTouchKeyboardAutoShow: (touchKeyboardAutoShow) => set({ touchKeyboardAutoShow }),
      setInputMode: (inputMode) => set({ inputMode }),
      setAudioOutputDevice: (audioOutputDevice) => set({ audioOutputDevice }),
      setFollowDefaultOutput: (followDefaultOutput) => set({ followDefaultOutput }),
      resetTheme: () =>
        set({
          accentColor: DEFAULTS.accentColor,
          bgPrimary: DEFAULTS.bgPrimary,
          themePreset: DEFAULTS.themePreset,
          backgroundImage: DEFAULTS.backgroundImage,
          backgroundOpacity: DEFAULTS.backgroundOpacity,
          backgroundDim: DEFAULTS.backgroundDim,
          backgroundBlur: DEFAULTS.backgroundBlur,
          glassBlur: DEFAULTS.glassBlur,
        }),
    }),
    {
      name: 'sc-settings',
      storage: createJSONStorage(() => tauriStorage),
      version: 20,
      migrate: (persistedState) => {
        const prev = (persistedState ?? {}) as Partial<SettingsState> & {
          soundwaveDiversity?: number;
          pinnedPlaylists?: SidebarPinnedPlaylist[];
        };
        // v13 → v14 migration kept for compatibility: drop the old
        // diversity slider value; SoundWave was removed entirely.
        // v19 → v20: pins moved from one shared list to per-account lists.
        // The old list is parked under `legacy` and claimed by the first
        // signed-in account (see `claimLegacyPins`).
        const legacy = prev.pinnedPlaylists ?? [];
        const byAccount = prev.pinnedPlaylistsByAccount ?? (legacy.length > 0 ? { legacy } : {});
        const migrated = { ...prev };
        delete (migrated as { pinnedPlaylists?: unknown }).pinnedPlaylists;
        return {
          ...DEFAULTS,
          ...migrated,
          pinnedPlaylistsByAccount: byAccount,
        } as SettingsState;
      },
      partialize: (s) => ({
        accentColor: s.accentColor,
        bgPrimary: s.bgPrimary,
        themePreset: s.themePreset,
        perfMode: s.perfMode,
        backgroundImage: s.backgroundImage,
        backgroundOpacity: s.backgroundOpacity,
        backgroundDim: s.backgroundDim,
        backgroundBlur: s.backgroundBlur,
        glassBlur: s.glassBlur,
        audioCacheLimitMB: s.audioCacheLimitMB,
        eqEnabled: s.eqEnabled,
        eqGains: s.eqGains,
        eqPreset: s.eqPreset,
        normalizeVolume: s.normalizeVolume,
        highQualityStreaming: s.highQualityStreaming,
        autoplayStation: s.autoplayStation,
        sidebarCollapsed: s.sidebarCollapsed,
        floatingComments: s.floatingComments,
        startupPage: s.startupPage,
        pinnedPlaylistsByAccount: s.pinnedPlaylistsByAccount,
        discordRpcEnabled: s.discordRpcEnabled,
        discordRpcMode: s.discordRpcMode,
        discordRpcShowButton: s.discordRpcShowButton,
        touchKeyboardAutoShow: s.touchKeyboardAutoShow,
        inputMode: s.inputMode,
        audioOutputDevice: s.audioOutputDevice,
        followDefaultOutput: s.followDefaultOutput,
      }),
    },
  ),
);
