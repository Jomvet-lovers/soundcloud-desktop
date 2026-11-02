import { create } from 'zustand';

/**
 * Session auto-recovery state.
 *
 * - `idle`   — all good, nothing to do.
 * - `silent` — the first automatic renew attempt is in flight. NO modal:
 *              everything must stay invisible to the user.
 * - `modal`  — auto-renew didn't help. Show the only modal, where renew can be
 *              retried manually or the user can sign in again (OAuth).
 *
 * `busy` — a manual renew is running inside the modal (spinner, the modal can't be closed).
 * The OAuth flow is tracked separately inside the modal itself (useOAuthFlow.isPolling).
 *
 * Transition logic lives in `lib/auth-recovery.ts`; this file holds state only.
 */
export type RecoveryPhase = 'idle' | 'silent' | 'modal';

interface AuthRecoveryState {
  phase: RecoveryPhase;
  busy: boolean;
  /** OAuth polling is running in the modal — leave success auto-close alone. */
  oauthActive: boolean;
  /** Timestamp of the last successful recovery — for cooldown. */
  recoveredAt: number;
  setPhase: (phase: RecoveryPhase) => void;
  setBusy: (busy: boolean) => void;
  setOauthActive: (oauthActive: boolean) => void;
  /** Success: dismiss the modal and stamp the cooldown. */
  markRecovered: () => void;
  /** Reset without cooldown (dismiss / logout). */
  reset: () => void;
}

export const useAuthRecoveryStore = create<AuthRecoveryState>((set) => ({
  phase: 'idle',
  busy: false,
  oauthActive: false,
  recoveredAt: 0,
  setPhase: (phase) => set({ phase }),
  setBusy: (busy) => set({ busy }),
  setOauthActive: (oauthActive) => set({ oauthActive }),
  markRecovered: () => set({ phase: 'idle', busy: false, recoveredAt: Date.now() }),
  reset: () => set({ phase: 'idle', busy: false }),
}));
