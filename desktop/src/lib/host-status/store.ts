import {create} from 'zustand';
import {API_BASE, API_STAR_BASE} from '../constants';

export type HostVerdict = 'up' | 'down' | 'unknown';
export type NetVerdict = 'unknown' | 'online' | 'no-internet';

export interface HostStatusState {
  main: HostVerdict;
  star: HostVerdict;
  net: NetVerdict;
  /** Активная проба идёт (single-flight гейт + фидбек retry-кнопки). */
  probing: boolean;
  /** ++ на подтверждённом переходе main→down при net=online. */
  incidentId: number;
  modalDismissedIncidentId: number;
  bannerDismissedIncidentId: number;
  lastModalDismissAt: number;
  dismissModal: () => void;
  dismissBanner: () => void;
  reopenModal: () => void;
}

// Без persist: статус аварии — runtime-only.
export const useHostStatusStore = create<HostStatusState>()((set, get) => ({
  main: 'unknown',
  star: 'unknown',
  net: 'unknown',
  probing: false,
  incidentId: 0,
  modalDismissedIncidentId: -1,
  bannerDismissedIncidentId: -1,
  lastModalDismissAt: 0,
  dismissModal: () =>
    set({ modalDismissedIncidentId: get().incidentId, lastModalDismissAt: Date.now() }),
  dismissBanner: () => set({ bannerDismissedIncidentId: get().incidentId }),
  reopenModal: () => set({ modalDismissedIncidentId: -1 }),
}));

export type FailoverUi = 'none' | 'star-active' | 'star-offer' | 'all-down';

/** Derived UI-состояние; UI его рендерит, нигде не хранится. backendReachable в гейте не участвует. */
export function selectFailoverUi(
  s: Pick<HostStatusState, 'main' | 'star' | 'net'>,
  premium: boolean,
  hasSession: boolean,
  navigatorOnline: boolean,
  offlineBypass: boolean,
): FailoverUi {
  if (!navigatorOnline || offlineBypass) return 'none'; // нет сети / юзер сам выбрал offline
  if (s.net === 'no-internet' || s.main !== 'down') return 'none';
  if (s.star === 'up') return premium && hasSession ? 'star-active' : 'star-offer';
  if (s.star === 'down') return 'all-down';
  return 'none'; // star='unknown': окно ранней пробы (≤10 c), не мигаем
}

export function getHostVerdict(base: string): HostVerdict {
  const s = useHostStatusStore.getState();
  if (base === API_BASE) return s.main;
  if (base === API_STAR_BASE) return s.star;
  return 'unknown';
}

export function isIncidentActive(): boolean {
  return useHostStatusStore.getState().main === 'down';
}

/** Control-plane база: ровно ОДИН хост, без перебора (refresh нельзя гонять по двум). */
export function preferredControlBase(): string {
  const s = useHostStatusStore.getState();
  return s.main !== 'down' ? API_BASE : s.star === 'up' ? API_STAR_BASE : API_BASE;
}
