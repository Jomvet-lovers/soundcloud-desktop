import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';
import en from './locales/en.json';
import ru from './locales/ru.json';
import tr from './locales/tr.json';

i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    ru: { translation: ru },
    tr: { translation: tr },
  },
  lng: navigator.language?.split('-')[0] || 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
});

// Sync language changes back to settings store
i18n.on('languageChanged', (lng) => {
  import('../stores/settings').then(({ useSettingsStore }) => {
    const store = useSettingsStore.getState();
    if (store.language !== lng) {
      store.setLanguage(lng);
    }
  });
});

export default i18n;
