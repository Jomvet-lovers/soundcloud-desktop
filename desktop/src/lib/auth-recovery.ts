import { useAuthStore } from '../stores/auth';
import { type RecoveryPhase, useAuthRecoveryStore } from '../stores/auth-recovery';
import { ApiError } from './api';
import { describeError, logInfo } from './diagnostics';
import { queryClient } from './query-client';

/**
 * Session recovery orchestrator.
 *
 * Triggers (from api-client):
 *   - `noteRateLimit()` — rate limit. We do NOT react to a single one: accumulator,
 *     modal only on sustained throttling (THRESHOLD per WINDOW).
 *   - `noteAuthGap()`   — stale token (401) OR the user vanished from the sidebar.
 *     A strong signal → silent renew at once.
 *   - `noteSuccess()`   — any successful response: clears the accumulator and, if
 *     everything recovered on its own, dismisses pending-recovery / closes the modal.
 *
 * Strategy: silent renew with no UI. The re-login modal — ONLY on 401 (SC rejected
 * refresh). A transient (5xx/429/timeout) means an unreachable host, not a dead
 * session: retry quietly with a growing delay and do NOT offer re-login — otherwise
 * a backend outage kicks out a live user. Single-flight via `inFlight` + `phase`.
 */

const RL_WINDOW_MS = 15_000;
const RL_THRESHOLD = 3;
const RECOVERED_COOLDOWN_MS = 5000;
const SILENT_RETRY_DELAY_MS = 2000;
/** Delay ceiling while the backend is down: it may be waited on for a long time, but not silently forever. */
const OUTAGE_RETRY_MAX_MS = 30_000;

/**
 * Is the session actually dead? Only a 401 is SoundCloud's verdict on the token.
 * Everything else (our backend's 5xx, timeout, connection drop) speaks to host
 * availability, not the session.
 */
function isSessionDead(e: unknown): boolean {
  return e instanceof ApiError && e.status === 401;
}

let rlHits: number[] = [];
let inFlight: Promise<void> | null = null;
/** Generation of the current silent attempt — to cancel on self-recovery. */
let gen = 0;
let cancelledGen = -1;
let silentAttempts = 0;
let retryTimer: ReturnType<typeof setTimeout> | null = null;
let lastSuccessAt = 0;

function clearRetryTimer(): void {
  if (retryTimer !== null) {
    clearTimeout(retryTimer);
    retryTimer = null;
  }
}

/**
 * The single point of phase change — and it is logged. Session recovery has been
 * completely invisible so far: from outside only the result shows ("modal present" /
 * "no modal"), and which transition caused its absence is visible from nowhere.
 * Every investigation hit this wall, so transitions are now in the log.
 */
function movePhase(phase: RecoveryPhase, why: string): void {
  const s = useAuthRecoveryStore.getState();
  logInfo(`[Recovery] ${s.phase} → ${phase} (${why})`);
  s.setPhase(phase);
}

async function runRenew(manual: boolean): Promise<void> {
  if (inFlight) return inFlight;
  clearRetryTimer();

  const myGen = ++gen;
  const store = useAuthRecoveryStore.getState();
  if (manual) {
    silentAttempts = 0;
    store.setBusy(true);
  } else {
    movePhase('silent', 'renew started');
  }

  inFlight = (async () => {
    try {
      await useAuthStore.getState().renewSession();
      silentAttempts = 0;
      clearRetryTimer();
      logInfo('[Recovery] renew succeeded → session is alive');
      useAuthRecoveryStore.getState().markRecovered();
      queryClient.invalidateQueries();
    } catch (e) {
      // Self-recovered via a parallel successful request — don't slap up the modal.
      if (cancelledGen === myGen) {
        logInfo('[Recovery] verdict discarded: an authenticated request went through first');
        return;
      }
      const s = useAuthRecoveryStore.getState();
      s.setBusy(false);
      // A refresh refusal comes in TWO kinds, and they must not be confused.
      // 401 — SC rejected the refresh: the session really is dead, re-login needed.
      // 5xx/timeout/transport — OUR backend is down: the session is fine, re-login
      // won't revive it but will kick the user out. Previously both landed here (two
      // arbitrary failures in a row → modal), and during a main outage the user was
      // offered a re-login out of nowhere.
      const needsReauth = isSessionDead(e);
      if (needsReauth) {
        silentAttempts = 0;
        movePhase('modal', `refresh rejected the session: ${describeError(e)}`);
      } else if (!manual) {
        // Backend is down — wait for it without touching the session. The delay grows
        // so we don't hammer a fallen host, but we keep a ceiling: the host will return on its own.
        silentAttempts++;
        movePhase('idle', `host unreachable, waiting to retry: ${describeError(e)}`);
        clearRetryTimer();
        const scheduledAt = Date.now();
        const delay = Math.min(SILENT_RETRY_DELAY_MS * silentAttempts, OUTAGE_RETRY_MAX_MS);
        retryTimer = setTimeout(() => {
          retryTimer = null;
          if (lastSuccessAt > scheduledAt) return; // came back on its own during the delay
          void runRenew(false);
        }, delay);
      }
    } finally {
      inFlight = null;
    }
  })();

  return inFlight;
}

function startRecovery(): void {
  const s = useAuthRecoveryStore.getState();
  if (s.phase !== 'idle') return; // renew already in flight or the modal already up
  if (Date.now() - s.recoveredAt < RECOVERED_COOLDOWN_MS) {
    logInfo('[Recovery] start suppressed: we just recovered successfully');
    return;
  }
  void runRenew(false);
}

/** Rate limit: accumulate, escalate only on sustained throttling. */
export function noteRateLimit(): void {
  const now = Date.now();
  rlHits.push(now);
  rlHits = rlHits.filter((t) => now - t < RL_WINDOW_MS);
  if (rlHits.length >= RL_THRESHOLD) {
    rlHits = [];
    startRecovery();
  }
}

/** Stale token / user vanished from the sidebar — react immediately. */
export function noteAuthGap(): void {
  startRecovery();
}

/**
 * Successful response: clear the accumulator and, if everything recovered on
 * its own, drop pending-recovery or auto-close the modal (but not during a manual
 * renew / OAuth — there the user is in charge).
 *
 * `authenticated` — whether the request carried `x-session-id`. "Session alive" can
 * ONLY be concluded from such a response. Previously any 200 went here, including
 * from public endpoints, and it quietly ate the verdict: renew under convoy runs
 * for tens of seconds (24 s in the logs), during which some public request got
 * through, set `cancelledGen` and dropped the phase to idle, while the 401 that
 * came back right after from refresh hit "recovered by itself" and exited
 * silently. The re-login modal was never shown — the user was left staring at an
 * empty screen.
 */
export function noteSuccess(authenticated: boolean): void {
  if (!authenticated) return;
  lastSuccessAt = Date.now();
  silentAttempts = 0;
  if (rlHits.length) rlHits = [];
  const s = useAuthRecoveryStore.getState();
  if (s.phase === 'idle' || s.busy || s.oauthActive) return;
  cancelledGen = gen;
  s.markRecovered();
}

/** Manual renew retry from the modal. */
export function retryRenew(): Promise<void> {
  return runRenew(true);
}

/** Successful full re-login (OAuth). */
export function completeReauth(sessionId: string): void {
  void (async () => {
    const auth = useAuthStore.getState();
    await auth.setSession(sessionId);
    await auth.fetchUser().catch(() => {});
    useAuthRecoveryStore.getState().markRecovered();
    queryClient.invalidateQueries();
  })();
}
