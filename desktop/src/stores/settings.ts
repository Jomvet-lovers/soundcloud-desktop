import { create } from 'zustand';
import { persist } from 'zustand/middleware';
import { EQ_PRESETS, eqGainsFor } from '../lib/equalizer';
import type { PerfMode } from '../lib/perf';
import { createThrottledJsonStorage } from '../lib/tauri-storage';

export type ThemePreset = 'soundcloud' | 'dark' | 'neon' | 'forest' | 'crimson' | 'custom';
export type StartupPage = 'home' | 'feed' | 'search' | 'library' | 'settings';
export type DiscordRpcMode = 'track' | 'artist' | 'activity';
export type InputMode = 'auto' | 'touch';
export interface SidebarPinnedPlaylist {
  urn: string;
  title: string;
  artworkUrl: string | null;
}

/** Equalizer profile created by the user. Factory presets stay in
 *  `EQ_PRESETS`; their edits live in `eqPresetEdits`. */
export interface EqProfile {
  id: string;
  label: string;
  gains: number[];
  /** Curve the profile was created with. Reset returns to this. */
  defaults: number[];
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
  /** User-created profiles, in creation order. */
  eqProfiles: EqProfile[];
  /** Factory presets the user has adjusted, keyed by preset id. */
  eqPresetEdits: Record<string, number[]>;
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
   *  on Linux); null = follow the system default. Restored at startup. */
  audioOutputDevice: string | null;
  /** Friendly name of the chosen endpoint, used only when the saved id no
   *  longer matches (re-paired Bluetooth, id drift). */
  audioOutputDeviceName: string | null;
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
  /** Apply a factory preset, an edited preset or a user profile. */
  selectEqProfile: (id: string) => void;
  /** Save the current curve as a new named profile and select it. */
  createEqProfile: (label: string) => void;
  deleteEqProfile: (id: string) => void;
  /** Restore the active profile to its default curve. */
  resetEqProfile: () => void;
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
  setAudioOutputDeviceName: (name: string | null) => void;
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
  eqProfiles: [] as EqProfile[],
  eqPresetEdits: {} as Record<string, number[]>,
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
  audioOutputDeviceName: null,
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
      selectEqProfile: (id) =>
        set((s) => {
          const gains = eqGainsFor(id, s.eqProfiles, s.eqPresetEdits);
          if (!gains) return {};
          // Picking a profile is an explicit "use this curve": turning the EQ
          // on saves a second trip to the power button.
          return { eqPreset: id, eqGains: gains, eqEnabled: true };
        }),
      createEqProfile: (label) =>
        set((s) => {
          const id = `user-${Date.now().toString(36)}`;
          return {
            eqProfiles: [
              ...s.eqProfiles,
              { id, label, gains: [...s.eqGains], defaults: [...s.eqGains] },
            ],
            eqPreset: id,
          };
        }),
      deleteEqProfile: (id) =>
        set((s) => {
          const eqProfiles = s.eqProfiles.filter((p) => p.id !== id);
          if (s.eqPreset !== id) return { eqProfiles };
          return {
            eqProfiles,
            eqPreset: 'flat',
            eqGains: [...EQ_PRESETS.flat.gains],
          };
        }),
      resetEqProfile: () =>
        set((s) => {
          const active = s.eqPreset;
          // Factory preset: drop the user's edit and restore the stock curve.
          const preset = EQ_PRESETS[active];
          if (preset) {
            const eqPresetEdits = { ...s.eqPresetEdits };
            delete eqPresetEdits[active];
            return { eqGains: [...preset.gains], eqPresetEdits };
          }
          // User profile: go back to the curve it was created with.
          const profile = s.eqProfiles.find((p) => p.id === active);
          if (profile) {
            const gains = [...(profile.defaults ?? profile.gains)];
            return {
              eqGains: gains,
              eqProfiles: s.eqProfiles.map((p) => (p.id === active ? { ...p, gains } : p)),
            };
          }
          return {};
        }),
      setEqBand: (index, gain) =>
        set((s) => {
          const eqGains = [...s.eqGains];
          eqGains[index] = gain;
          const active = s.eqPreset;
          if (s.eqProfiles.some((p) => p.id === active)) {
            // Editing a user profile writes through to the profile itself, so
            // the change survives switching to another profile and back.
            return {
              eqGains,
              eqProfiles: s.eqProfiles.map((p) => (p.id === active ? { ...p, gains: eqGains } : p)),
            };
          }
          if (EQ_PRESETS[active]) {
            return { eqGains, eqPresetEdits: { ...s.eqPresetEdits, [active]: eqGains } };
          }
          return { eqGains };
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
      setAudioOutputDeviceName: (audioOutputDeviceName) => set({ audioOutputDeviceName }),
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
      name: 'sc-settings-v21',
      // Slider drags (EQ, wallpaper) fire dozens of writes per second; the
      // throttled adapter coalesces them and flushes when the window hides.
      storage: createThrottledJsonStorage(),
      version: 23,
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
        // v21 → v22: a curve left in the transient `custom` state becomes a
        // real profile, so it survives later profile switches.
        // v22 → v23: profiles remember the curve they were created with, which
        // is what the Reset button restores.
        const eqGains = prev.eqGains ?? DEFAULT_EQ_GAINS;
        // Older files predate `defaults`.
        type StoredEqProfile = Omit<EqProfile, 'defaults'> & { defaults?: number[] };
        let eqProfiles: StoredEqProfile[] = prev.eqProfiles ?? [];
        const eqPreset = prev.eqPreset ?? 'flat';
        if (eqPreset === 'custom' && !eqProfiles.some((p) => p.id === 'custom')) {
          eqProfiles = [
            ...eqProfiles,
            {
              id: 'custom',
              label: 'Custom',
              gains: [...eqGains],
              defaults: [...eqGains],
            },
          ];
        }
        const migratedProfiles: EqProfile[] = eqProfiles.map((p) => ({
          ...p,
          defaults: p.defaults ?? [...p.gains],
        }));
        return {
          ...DEFAULTS,
          ...migrated,
          pinnedPlaylistsByAccount: byAccount,
          eqProfiles: migratedProfiles,
          eqPreset,
          eqPresetEdits: prev.eqPresetEdits ?? {},
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
        eqProfiles: s.eqProfiles,
        eqPresetEdits: s.eqPresetEdits,
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
        audioOutputDeviceName: s.audioOutputDeviceName,
        followDefaultOutput: s.followDefaultOutput,
      }),
    },
  ),
);
