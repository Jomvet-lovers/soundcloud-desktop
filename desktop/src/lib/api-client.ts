import { toast } from 'sonner';
import { useAppStatusStore } from '../stores/app-status';
import { useAuthStore } from '../stores/auth';
import { noteAuthGap, noteRateLimit, noteSuccess } from './auth-recovery';
import { API_BASE, API_STAR_BASE } from './constants';
import { logHttpError, logHttpFailure, logInfo, trackAsync } from './diagnostics';
import { edgeFetch } from './edge';
import {
  getHostVerdict,
  isHealthy,
  isIncidentActive,
  isMainDegraded,
  isTimeoutError,
  markHealthy,
  markMainDegraded,
  markUnhealthy,
  noteMainBadResponse,
  noteRequestTimeout,
  preferredControlBase,
  SLOW_RESPONSE_MS,
} from './host-status';
import { getIsPremium, requestPremiumRecheck } from './premium-cache';

// ─── Session ────────────────────────────────────────────────

let sessionId: string | null = null;

/**
 * Exactly one place fills the session mirror — `applyAuthFromServer`, and until
 * that moment an authenticated request is doomed: it goes out without
 * `x-session-id` and gets a 401 "Missing or malformed". On startup that burned
 * `/history` and `/tracks` from the cache-restored player — two guaranteed 401s
 * in the log before the app even knew whether the user was signed in at all.
 *
 * So the first request waits until the bridge answers at least something
 * (including "no session" — that is an answer too). The wait has a ceiling: the
 * tray has its own webview with no bridge, and without a ceiling its requests
 * would hang forever.
 */
let sessionKnown = false;
let announceSessionKnown: () => void = () => {};
const sessionKnownPromise = new Promise<void>((resolve) => {
  announceSessionKnown = resolve;
});
const SESSION_WAIT_MS = 3_000;

export function setSessionId(id: string | null) {
  sessionId = id;
  if (!sessionKnown) {
    sessionKnown = true;
    announceSessionKnown();
  }
}

function awaitSessionKnown(): Promise<unknown> {
  if (sessionKnown) return Promise.resolve();
  return Promise.race([
    sessionKnownPromise,
    new Promise((resolve) => setTimeout(resolve, SESSION_WAIT_MS)),
  ]);
}

export function getSessionId() {
  return sessionId;
}

// ─── Error ──────────────────────────────────────────────────

export class ApiError extends Error {
  constructor(
    public status: number,
    public body: string,
  ) {
    super(`API ${status}: ${body}`);
    this.name = 'ApiError';
  }
}

/**
 * The server's reason out of a failed `api()` call, or null when the body
 * has none. The wrapper message is `API <status>: <json body>`; direct-mode
 * errors carry `{"error": "..."}`.
 */
export function apiErrorReason(error: unknown): string | null {
  if (!(error instanceof Error)) return null;
  const m = /^API \d+:\s*(\{.*\})\s*$/.exec(error.message);
  if (!m) return null;
  try {
    const parsed = JSON.parse(m[1]) as { error?: unknown };
    if (typeof parsed.error === 'string' && parsed.error) return parsed.error;
  } catch {
    /* non-JSON body */
  }
  return null;
}

function isRateLimitError(status: number, body: string): boolean {
  if (status === 429) return true;
  const b = body.toLowerCase();
  return b.includes('rate limit') || b.includes('rate-limited') || b.includes('too many requests');
}

// ─── Helpers ────────────────────────────────────────────────

function fetchWithTimeout(url: string, options: RequestInit, timeoutMs: number): Promise<Response> {
  return edgeFetch(url, options, timeoutMs);
}

function handleApiError(err: ApiError): void {
  if (err.status >= 500) {
    if (isIncidentActive()) return; // incident already shown by modal/banner
    // Fixed id: sonner replaces the toast, so a storm doesn't stack.
    toast.error(`Server error (${err.status})`, { id: 'api-server-error' });
  } else if (err.status >= 400 && err.status !== 401) {
    try {
      const parsed = JSON.parse(err.body);
      toast.error(parsed.message || parsed.error || `Error ${err.status}`);
    } catch {
      toast.error(`Error ${err.status}`);
    }
  }
}

// ─── Main API client ────────────────────────────────────────

export type ApiRequestOptions = RequestInit & {
  /**
   * HTTP statuses considered normal: no error toast, no auth/rate-limit
   * recovery and no error log. ApiError is still thrown — quietly, so the caller
   * can reduce it to a default (e.g. 404 /related → empty related list).
   */
  silentStatuses?: number[];
};

/**
 * Budgets — measured in prod (2026-07-30), not eyeballed. There is one reason
 * authenticated endpoints are slow: on the backend `get_valid_session` holds a
 * per-session mutex while it fetches a new token from SoundCloud when the token
 * is stale. The client fires dozens of requests at that moment, and all of them
 * queue up behind it. So "slow" here is normal behavior, not a sign the host is dead.
 *
 * `/auth/*`  — the backend round-trips to SoundCloud: login measured 4.2–11.7 s.
 * `/me/subscription` — the endpoint itself is one SELECT, but under the convoy of
 *   the refresh lock legit 17.9 s shows up in the logs.
 * data-plane — `/recommendations` got as high as 49.8 s.
 *
 * A host already declared dead waits none of this: `DOWN_HOST_TIMEOUT_MS`.
 */
const AUTH_TIMEOUT_MS = 30_000;
const SUBSCRIPTION_TIMEOUT_MS = 30_000;
const DATA_PLANE_TIMEOUT_MS = 90_000;
const DOWN_HOST_TIMEOUT_MS = 10_000;

/**
 * Has main fallen back to the safety-net role? A probe verdict alone isn't
 * enough: 'down' is set from `/health`, and it answers even when the host no
 * longer serves anything (see `host-status/degraded.ts`). Degradation is the
 * same routing verdict, but only for those star will answer: a non-premium user
 * gets a 403 there, and sending them there trades a slow answer for a fast
 * refusal.
 */
function mainIsLastResort(): boolean {
  if (getHostVerdict(API_BASE) === 'down') return true;
  return isMainDegraded() && getIsPremium();
}

/**
 * Request bases, primary first.
 * /me/subscription — open on star (premium bootstrap signal): iterate both, primary by main's state.
 * /auth/login* — pinned to main: the OAuth redirect_uri is registered with SC only on the primary host.
 * the rest of /auth/* (refresh, link/*, logout) — exactly ONE host per verdict: refresh must not be
 *   failed over by iteration (per-process refresh_locks on the backend → double refresh_token rotation),
 *   link tokens are single-use.
 * data-plane — premium → star, main as the safety net. GET/HEAD fail over freely, mutations
 * know main as the safety net but switch there only on an explicit
 * infrastructure HTML error (the request never reached the app).
 */
function apiBasesFor(path: string): string[] {
  if (path === '/me/subscription') {
    return mainIsLastResort() ? [API_STAR_BASE, API_BASE] : [API_BASE, API_STAR_BASE];
  }
  if (path.startsWith('/auth/login')) return [API_BASE];
  if (path.startsWith('/auth/refresh')) {
    // The only control-plane mutation that can be replayed on a neighbor, and it
    // must be: a 401 from here is the "session is dead" verdict, after which the
    // user is shown a re-login. The session is SHARED between hosts, so a 401
    // from one while the session is alive means "this host is broken", not "the
    // session died". Checking the other verdict before kicking someone out of
    // their account is the bare minimum of decency. Replay strictly on 401 (see
    // `canReplayOnNextHost`).
    const primary = preferredControlBase();
    return [primary, primary === API_BASE ? API_STAR_BASE : API_BASE];
  }
  if (path.startsWith('/auth/')) return [preferredControlBase()];
  if (!getIsPremium() || !sessionId) return [API_BASE];
  // star's cooldown moves premium to main only while main can still answer.
  // Otherwise star stays first: its 30-second timeout is one miss, not a
  // verdict, and replaying it is cheaper than joining the convoy on main.
  // Previously this was `[API_BASE]` with no star at all — any half-minute miss
  // on star pinned the subscriber back to main, even when main was choking.
  return isHealthy(API_STAR_BASE) || mainIsLastResort()
    ? [API_STAR_BASE, API_BASE]
    : [API_BASE, API_STAR_BASE];
}

/** Request budget by path. */
function planeTimeout(path: string): number {
  if (path.startsWith('/auth/')) return AUTH_TIMEOUT_MS;
  if (path === '/me/subscription') return SUBSCRIPTION_TIMEOUT_MS;
  return DATA_PLANE_TIMEOUT_MS;
}

/**
 * Sample for the main-degradation detector. A failure is always bad; a slow
 * response only outside `/auth/*`: there the backend round-trips to SoundCloud,
 * and the 4.2–11.7 s measured on login is the handler's norm, not host state.
 */
function isMainBadSample(path: string, ok: boolean, elapsedMs: number): boolean {
  if (!ok) return true;
  return !path.startsWith('/auth/') && elapsedMs >= SLOW_RESPONSE_MS;
}

/** Short host name for failover logs. */
function hostLabel(base: string): string {
  return base === API_BASE ? 'main' : base === API_STAR_BASE ? 'star' : base;
}

/** Host failure → failover. 4xx contracts (400/404/…) are a valid answer, don't fail over. */
function isHostFailover(status: number): boolean {
  return status >= 500 || status === 401 || status === 403;
}

/**
 * Load-balancer/proxy failures are safe to replay even for a mutation: the
 * HTML error page means the request never reached the API application. JSON
 * 5xx responses remain application errors and are not replayed.
 */
function isInfrastructureFailure(status: number, body: string): boolean {
  if (status < 500) return false;
  const b = body.toLowerCase();
  return b.includes('<html') || b.includes('no server is available');
}

/**
 * Can THIS request be retried on the next host? GET/HEAD — always, an
 * infrastructure failure — always (it never reached the app).
 *
 * A special case is a 401 on `/auth/refresh`. Refresh is pinned to one host
 * precisely because of the double refresh_token rotation risk, but the 401
 * comes from the session lookup BEFORE any rotation: `Backend/api/src/modules/auth/service.rs`,
 * `refresh_session` — `get_session` → 401 "Session not found", and only then
 * the lock and `do_refresh`. No rotation happened, so replaying is safe.
 * Timeout and 5xx are still off-limits: the request may have arrived and applied.
 */
function canReplayOnNextHost(path: string, method: string, status: number, body: string): boolean {
  if (method === 'GET' || method === 'HEAD') return true;
  if (isInfrastructureFailure(status, body)) return true;
  return path.startsWith('/auth/refresh') && status === 401;
}

export async function apiRequest<T = unknown>(
  path: string,
  options: ApiRequestOptions = {},
  timeoutMs?: number,
): Promise<T> {
  const { silentStatuses, ...init } = options;
  // The bridge may not have reached the mirror yet — otherwise the request
  // goes out without a session, into the void.
  if (!sessionKnown) await awaitSessionKnown();
  const headers = new Headers(init.headers);
  // Guard against the string "undefined"/"null" landing in the header during API format upgrades.
  const authenticated = !!sessionId && sessionId !== 'undefined' && sessionId !== 'null';
  if (authenticated) {
    headers.set('x-session-id', sessionId as string);
  }
  if (!headers.has('Content-Type') && init.body) headers.set('Content-Type', 'application/json');

  const method = init.method ?? 'GET';
  const label = `${method.toUpperCase()} ${path}`;
  const bases = apiBasesFor(path);
  const effectiveTimeout = timeoutMs ?? planeTimeout(path);
  let lastError: unknown = null;
  /** main refused this session — wait to see what the neighbor says about it. */
  let mainRefused = false;
  /** First 401 in the failover: it delivers the session verdict, not star's gate 403. */
  let authRejection: ApiError | null = null;

  for (let i = 0; i < bases.length; i++) {
    const base = bases[i];
    const isLast = i === bases.length - 1;
    const url = `${base}${path}`;
    const attemptStart = performance.now();

    // A host with a down verdict holds an attempt no longer than 10 s.
    const attemptTimeout =
      getHostVerdict(base) === 'down'
        ? Math.min(effectiveTimeout, DOWN_HOST_TIMEOUT_MS)
        : effectiveTimeout;

    try {
      const res = await trackAsync(
        `http:${label}`,
        fetchWithTimeout(url, { ...init, headers }, attemptTimeout),
      );

      // Alive = answered <500 (like probeOnce; 401/403 are a valid axum answer and
      // do NOT taint star — otherwise a stale token would disable star while main is dead).
      // ≥500 — passive failure: cooldown + main probe; only the probe sets the down verdict.
      if (res.status < 500) markHealthy(base);
      else markUnhealthy(base);
      if (
        base === API_BASE &&
        isMainBadSample(path, res.status < 500, performance.now() - attemptStart)
      ) {
        noteMainBadResponse();
      }
      // star success for a non-premium user is a probe signal, not "online"
      // (otherwise offline↔online flaps).
      if (base === API_BASE || getIsPremium()) {
        useAppStatusStore.getState().setBackendReachable(true);
      }

      // The neighbor served on the SAME session what main just refused. The
      // session is shared — so main is broken, not the session. Proof, not a sample:
      // we move all traffic there at once, without filling a window.
      if (res.ok && mainRefused && base !== API_BASE) {
        markMainDegraded();
        logInfo(`[Host] main refused ${label}; star served the same session → using star`);
      }

      if (!res.ok) {
        const body = await res.text();
        const err = new ApiError(res.status, body);

        // Expected star gate refusal (non-premium): not noise but suspicion —
        // the recheck request doesn't trigger itself.
        const starDeny = base === API_STAR_BASE && res.status === 403;
        if (starDeny && path !== '/me/subscription') requestPremiumRecheck();

        // Contractually normal status (e.g. 404 /related = no neighbors yet):
        // silenced quietly — no toast, no recovery, no error log.
        if (silentStatuses?.includes(res.status)) throw err;

        if (res.status === 401) authRejection ??= err;

        if (
          !isLast &&
          isHostFailover(res.status) &&
          canReplayOnNextHost(path, method, res.status, body)
        ) {
          lastError = err;
          if (base === API_BASE) mainRefused = true;
          // Intermediate refusals never made it into the log at all before
          // (logHttpError is only called on the last base). From outside that read
          // as "requests don't go to star", yet they went there first — just
          // silently. Failover must be visible or it can't be debugged.
          logInfo(`[Host] ${label}: ${hostLabel(base)} → ${res.status}, trying the next host`);
          // The neighbor will answer on the same session now — so a 401 here is not
          // yet a reason to wake recovery. We wake it on the last host if that one
          // refuses too; otherwise one broken host would kick the user out alone.
          continue;
        }

        // star's gate 403 means "host isn't yours", not a session answer. If main
        // said 401 before it, that 401 delivers the verdict: otherwise a reserve-
        // host access refusal would mask a dead session, and a re-login would
        // never be offered — the user would be left with an eternally empty screen.
        const verdict = starDeny && authRejection ? authRejection : err;

        logHttpError(label, verdict.status, url, verdict.body);

        // Rate limit — accumulate it; a single one doesn't trigger recovery.
        if (isRateLimitError(verdict.status, verdict.body)) {
          noteRateLimit();
          console.error(`HTTP ERROR: url: ${path}, `, verdict);
          throw verdict;
        }

        // Stale token (401) or the user vanished from the sidebar — a strong
        // signal, silent renew at once. star's gate 403 doesn't trigger recovery.
        //
        // 5xx does NOT land here: "server broke" doesn't mean "session died".
        // On a cold start `user` is still null, and a backend 502 (including the
        // normal "Renewing your session, try again shortly") went to recovery, and
        // after two silent attempts — to the "session expired" modal.
        const looksLikeAuthGap =
          verdict.status === 401 ||
          (verdict.status < 500 && useAuthStore.getState().user == null && !starDeny);
        if (looksLikeAuthGap) {
          noteAuthGap();
          console.error(`HTTP ERROR: url: ${path}, `, verdict);
          throw verdict;
        }

        if (!starDeny) handleApiError(err);
        console.error(`HTTP ERROR: url: ${path}, `, err);
        throw err;
      }

      // A successful response clears the rate-limit accumulator and self-dismisses
      // recovery if everything came back on its own. Only authenticated: a public
      // 200 says nothing about the session and must not lift the verdict.
      noteSuccess(authenticated);

      const ct = res.headers.get('content-type');
      const reply = await (ct?.includes('application/json') ? res.json() : (res.text() as T));

      if (typeof reply === 'string') {
        try {
          return JSON.parse(reply) as T;
        } catch {}
      }

      return reply;
    } catch (error) {
      if (error instanceof ApiError) throw error;
      markUnhealthy(base);
      if (base === API_BASE) noteMainBadResponse();
      if (isTimeoutError(error)) noteRequestTimeout();
      if (!isLast) {
        lastError = error;
        logInfo(`[Host] ${label}: ${hostLabel(base)} did not respond, trying the next host`);
        continue;
      }
      logHttpFailure(label, url, error, performance.now() - attemptStart);
      useAppStatusStore.getState().setBackendReachable(false);
      throw error;
    }
  }

  throw lastError ?? new Error('Request failed');
}

// ─── Aliases ────────────────────────────────────────────────

export const fetchWithAuthFallback = apiRequest;
