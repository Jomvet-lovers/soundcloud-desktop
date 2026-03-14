import { create } from 'zustand';
import { createJSONStorage, persist } from 'zustand/middleware';
import { tauriStorage } from '../lib/tauri-storage';

export interface SettingsState {
  accentColor: string;
  backgroundImage: string;
  backgroundOpacity: number;
  glassBlur: number;
  language: string;
  setAccentColor: (color: string) => void;
  setBackgroundImage: (url: string) => void;
  setBackgroundOpacity: (opacity: number) => void;
  setGlassBlur: (blur: number) => void;
  setLanguage: (lang: string) => void;
  resetTheme: () => void;
}

const DEFAULTS = {
  accentColor: '#ff5500',
  backgroundImage: '',
  backgroundOpacity: 0.15,
  glassBlur: 40,
  language: navigator.language?.split('-')[0] || 'en',
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULTS,
      setAccentColor: (accentColor) => set({ accentColor }),
      setBackgroundImage: (backgroundImage) => set({ backgroundImage }),
      setBackgroundOpacity: (backgroundOpacity) => set({ backgroundOpacity }),
      setGlassBlur: (glassBlur) => set({ glassBlur }),
      setLanguage: (language) => set({ language }),
      resetTheme: () => set(DEFAULTS),
    }),
    {
      name: 'sc-settings',
      storage: createJSONStorage(() => tauriStorage),
      version: 2,
      partialize: (s) => ({
        accentColor: s.accentColor,
        backgroundImage: s.backgroundImage,
        backgroundOpacity: s.backgroundOpacity,
        glassBlur: s.glassBlur,
        language: s.language,
      }),
      onRehydrateStorage: () => (state) => {
        if (!state) return;
        // Sync language with i18n on hydration
        import('../i18n').then(({ default: i18n }) => {
          if (state.language && state.language !== i18n.language) {
            i18n.changeLanguage(state.language);
          }
        });
      },
    },
  ),
);
