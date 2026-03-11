import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import { readTextFile, writeTextFile, exists, BaseDirectory } from '@tauri-apps/plugin-fs';
import en from './locales/en.json';
import ru from './locales/ru.json';

const LANG_FILE = 'language.txt';
const BASE_DIR = BaseDirectory.AppData;

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    ru: { translation: ru },
  },
  lng: navigator.language?.split('-')[0] || 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
});

// Restore saved language from file (overrides browser detection)
(async () => {
  try {
    if (await exists(LANG_FILE, { baseDir: BASE_DIR })) {
      const saved = (await readTextFile(LANG_FILE, { baseDir: BASE_DIR })).trim();
      if (saved && saved !== i18n.language) {
        await i18n.changeLanguage(saved);
      }
    }
  } catch {
    // first run — use browser language
  }
})();

// Persist language changes to file
i18n.on('languageChanged', (lng) => {
  writeTextFile(LANG_FILE, lng, { baseDir: BASE_DIR }).catch(() => {});
});

export default i18n;