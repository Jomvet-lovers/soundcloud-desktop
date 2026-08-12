import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react';
import { en } from './en';
import { ru, type I18nKey } from './ru';

export type Locale = 'ru' | 'en';

const dictionaries: Record<Locale, Record<I18nKey, string>> = { ru, en };

type T = (key: I18nKey) => string;

const I18nCtx = createContext<T | null>(null);

/** Строки текущей локали. Провайдер — `I18nProvider` (смонтирован в `AppShell`). */
export function useT(): T {
  const t = useContext(I18nCtx);
  if (!t) throw new Error('I18nProvider отсутствует выше по дереву');
  return t;
}

/** Минимальный i18n: локаль в стейте, плоский словарь без интерполяции/множественных
 *  форм. Дефолт — `ru` (приложение русскоязычное); явный переключатель локали появится
 *  с экраном настроек, `en`-словарь держим наготове. */
export function I18nProvider({ children }: { children: ReactNode }) {
  const [locale] = useState<Locale>('ru');
  const t = useCallback<T>((key) => dictionaries[locale][key] ?? key, [locale]);
  const value = useMemo(() => t, [t]);
  return <I18nCtx.Provider value={value}>{children}</I18nCtx.Provider>;
}
