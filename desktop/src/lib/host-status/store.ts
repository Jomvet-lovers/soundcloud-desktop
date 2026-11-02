import { create } from 'zustand';
import { API_BASE, API_STAR_BASE } from '../constants';
import { getIsPremium } from '../premium-cache';
import { isMainDegraded } from './degraded';

export type HostVerdict = 'up' | 'down' | 'unknown';
export type NetVerdict = 'unknown' | 'online' | 'no-internet';

export interface HostStatusState {
  main: HostVerdict;
  star: HostVerdict;
  net: NetVerdict;
  /** An active probe is under way (single-flight gate + retry-button feedback) */
  probing: boolean;
  /** ++ on a confirmed main→down transition with net=online */
  incidentId: number;
  modalDismissedIncidentId: number;
  bannerDismissedIncidentId: number;
  lastModalDismissAt: number;
  dismissModal: () => void;
  dismissBanner: () => void;
  reopenModal: () => void;
}

// No persist: incident status is runtime-only
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

/** Derived UI state; the UI renders it, it is never stored. backendReachable takes no part in the gate */
export function selectFailoverUi(
  s: Pick<HostStatusState, 'main' | 'star' | 'net'>,
  premium: boolean,
  hasSession: boolean,
  navigatorOnline: boolean,
  offlineBypass: boolean,
  mainDegraded: boolean,
): FailoverUi {
  if (!navigatorOnline || offlineBypass) return 'none'; // no network / the user picked offline themselves
  if (s.net === 'no-internet') return 'none';
  if (s.main !== 'down') {
    // main is formally alive but not serving, and premium has already moved to star.
    // Silence is not an option here: from the outside it is «half the screens empty and nobody
    // said anything» — indistinguishable from a broken app.
    return mainDegraded && premium && hasSession && s.star !== 'down' ? 'star-active' : 'none';
  }
  if (s.star === 'up') return premium && hasSession ? 'star-active' : 'star-offer';
  if (s.star === 'down') return 'all-down';
  return 'none'; // star='unknown': early-probe window (≤10 s), no blinking
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

/**
 * Control-plane base: exactly ONE host, no iteration (refresh must not be run against two).
 *
 * For premium, a degraded main is as good as a dead one: its renew lands in the same
 * convoy as everything else, and star is available. Non-premium is not sent there even
 * then: star answers it with 403, while main is slow but does answer — there is no
 * reason to trade a slow answer for a fast refusal. A dead main is another matter: there the choice
 * is between 403 and a guaranteed failure, and we leave that behaviour alone.
 */
export function preferredControlBase(): string {
  const s = useHostStatusStore.getState();
  if (s.main === 'down') return s.star === 'up' ? API_STAR_BASE : API_BASE;
  if (isMainDegraded() && getIsPremium() && s.star !== 'down') return API_STAR_BASE;
  return API_BASE;
}
