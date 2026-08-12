import { createContext, useContext, useMemo, useState, type ReactNode } from 'react';

export type PanelId = 'queue' | 'eq';

interface Panels {
  open: PanelId | null;
  toggle: (p: PanelId) => void;
  close: () => void;
  isOpen: (p: PanelId) => boolean;
}

const Ctx = createContext<Panels | null>(null);

export const usePanels = (): Panels => {
  const c = useContext(Ctx);
  if (!c) throw new Error('PanelsProvider отсутствует выше по дереву');
  return c;
};

/** Оверлейные панели плеера (очередь/эквалайзер) — открыт максимум один. */
export function PanelsProvider({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState<PanelId | null>(null);
  const value = useMemo<Panels>(
    () => ({
      open,
      toggle: (p) => setOpen((cur) => (cur === p ? null : p)),
      close: () => setOpen(null),
      isOpen: (p) => open === p,
    }),
    [open],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
