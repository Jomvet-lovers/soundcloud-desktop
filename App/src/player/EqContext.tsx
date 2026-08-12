import { createContext, useCallback, useContext, useEffect, useMemo, type ReactNode } from 'react';
import { usePersistedState, useSc } from '@sc/data';
import { EQ_FLAT, EQ_PRESETS } from './eq-presets';

/** Состояние эквалайзера (донор settings-стор eq-часть). Персист — через генерик
 *  `usePersistedState` (ядро хранит по ключу `eq`, файл в app-storage). На любое
 *  изменение и на восстановление сохранённого пушим в ядро `set_eq`. */
export interface EqState {
  enabled: boolean;
  gains: number[];
  preset: string;
  setEnabled: (enabled: boolean) => void;
  setBand: (index: number, gain: number) => void;
  applyPreset: (id: string) => void;
  reset: () => void;
}

interface EqBlob {
  enabled: boolean;
  gains: number[];
  preset: string;
}
const DEFAULT_EQ: EqBlob = { enabled: false, gains: EQ_FLAT, preset: 'flat' };

const EqCtx = createContext<EqState | null>(null);

export const useEq = (): EqState => {
  const ctx = useContext(EqCtx);
  if (!ctx) throw new Error('EqProvider отсутствует выше по дереву');
  return ctx;
};

export function EqProvider({ children }: { children: ReactNode }) {
  const sc = useSc();
  const [eq, setEq] = usePersistedState<EqBlob>('eq', DEFAULT_EQ);

  // Пуш в ядро на любое изменение (и на приход сохранённого значения).
  useEffect(() => {
    void sc.player.setEq(eq.enabled, eq.gains);
  }, [sc, eq.enabled, eq.gains]);

  const setEnabled = useCallback((enabled: boolean) => setEq((p) => ({ ...p, enabled })), [setEq]);
  const setBand = useCallback(
    (index: number, gain: number) =>
      setEq((p) => {
        const gains = [...p.gains];
        gains[index] = gain;
        return { ...p, gains, preset: 'custom' };
      }),
    [setEq],
  );
  const applyPreset = useCallback(
    (id: string) => {
      const p = EQ_PRESETS[id];
      if (p) setEq((prev) => ({ ...prev, gains: [...p.gains], preset: id }));
    },
    [setEq],
  );
  const reset = useCallback(() => setEq((p) => ({ ...p, gains: [...EQ_FLAT], preset: 'flat' })), [setEq]);

  const value = useMemo<EqState>(
    () => ({ enabled: eq.enabled, gains: eq.gains, preset: eq.preset, setEnabled, setBand, applyPreset, reset }),
    [eq.enabled, eq.gains, eq.preset, setEnabled, setBand, applyPreset, reset],
  );

  return <EqCtx.Provider value={value}>{children}</EqCtx.Provider>;
}
