import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { tauriStorage } from '../lib/tauri-storage';

export interface SettingsState {
  accentColor: string;
  backgroundImage: string;
  backgroundOpacity: number;
  glassBlur: number;
  setAccentColor: (color: string) => void;
  setBackgroundImage: (url: string) => void;
  setBackgroundOpacity: (opacity: number) => void;
  setGlassBlur: (blur: number) => void;
  resetTheme: () => void;
}

const DEFAULTS = {
  accentColor: '#ff5500',
  backgroundImage: '',
  backgroundOpacity: 0.15,
  glassBlur: 40,
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULTS,
      setAccentColor: (accentColor) => set({ accentColor }),
      setBackgroundImage: (backgroundImage) => set({ backgroundImage }),
      setBackgroundOpacity: (backgroundOpacity) => set({ backgroundOpacity }),
      setGlassBlur: (glassBlur) => set({ glassBlur }),
      resetTheme: () => set(DEFAULTS),
    }),
    {
      name: 'sc-settings',
      storage: createJSONStorage(() => tauriStorage),
      partialize: (s) => ({
        accentColor: s.accentColor,
        backgroundImage: s.backgroundImage,
        backgroundOpacity: s.backgroundOpacity,
        glassBlur: s.glassBlur,
      }),
    },
  ),
);
