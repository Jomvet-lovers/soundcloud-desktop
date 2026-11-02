// «main is reachable, but not serving» — a state the probe does not have.
//
// Only `/health` moves the main verdict, and that endpoint is cheap on the backend: it does
// not take the per-session refresh mutex, so it answers fast even when every
// authorized endpoint is queued in the convoy behind it. So `main === 'down'` in the most
// common incident NEVER fires — the host is «alive», it just cannot keep up.
// And the whole switchover hangs off 'down': control-plane, `/me/subscription`,
// the order of the premium bases. Net effect — subscribers kept loading the half-dead main,
// although they have star, and the convoy on main also squeezed those with star closed.
//
// A second signal accumulates here, from REAL requests to main: a failure or a response
// slower than `SLOW_RESPONSE_MS`. `THRESHOLD` of them within `WINDOW_MS` → main
// is degraded for `COOLDOWN_MS`. A success does not clear the verdict: under a convoy some
// requests do go through normally, that is what a convoy is. The degradation expires on its
// own — it is a half-open check, and if main is still bad it arms again.

const WINDOW_MS = 60_000;
const THRESHOLD = 3;
const COOLDOWN_MS = 60_000;
/**
 * How long after the degradation expires ONE bad response is enough to bring
 * the verdict back. The half-open check asks «fixed?», and one answer
 * of «no» is exhaustive; otherwise every minute of the incident costs premium three
 * requests sent back into the convoy.
 */
const RE_ARM_MS = 120_000;

/**
 * A response slower than this is no longer «slow», it is «not serving». Prod measurements
 * under the refresh-lock convoy (30.07.2026): `/me/subscription` 17.9 s even though
 * the endpoint itself is a single SELECT; `/recommendations` — 49.8 s.
 */
export const SLOW_RESPONSE_MS = 12_000;

let badSamples: number[] = [];
let degradedUntil = 0;

/** main answers, but cannot serve — premium should move to star */
export function isMainDegraded(): boolean {
  return Date.now() < degradedUntil;
}

/**
 * A verdict without filling the window — for proof, not for a sample. The only
 * such case: star served what main refused, on a session shared by both hosts.
 * This is not «slow» and not «bad luck», it is a confirmed main breakage,
 * and there is no point waiting for two more samples.
 */
export function markMainDegraded(): void {
  badSamples = [];
  setDegradedUntil(Date.now() + COOLDOWN_MS);
}

/**
 * A real request to main failed or dragged on past `SLOW_RESPONSE_MS`.
 * What counts as a bad response is the caller's call: it alone knows the path, and
 * for `/auth/*` a slow response is a backend round-trip to SoundCloud, not degradation.
 */
export function noteMainBadResponse(): void {
  const now = Date.now();
  badSamples = badSamples.filter((t) => now - t < WINDOW_MS);
  badSamples.push(now);
  if (badSamples.length < currentThreshold(now)) return;
  // Window filled — start counting afresh so old samples do not extend the verdict
  // retroactively after it has already run its course.
  badSamples = [];
  setDegradedUntil(now + COOLDOWN_MS);
}

/** While the degradation is fresh (running or just expired) the threshold is lower. */
function currentThreshold(now: number): number {
  return now - degradedUntil < RE_ARM_MS ? 1 : THRESHOLD;
}

// ─── UI subscription ────────────────────────────────────────
// The user must not be silently moved to the backup host: «empty and nothing said»
// is indistinguishable from a broken app. The banner reads the verdict from here.

const listeners = new Set<() => void>();
let expiryTimer: ReturnType<typeof setTimeout> | null = null;

function setDegradedUntil(ts: number): void {
  degradedUntil = ts;
  if (expiryTimer !== null) clearTimeout(expiryTimer);
  // The degradation ends by the clock, not by an event: without an alarm the subscriber
  // would only learn about it on the next request — which may be never.
  expiryTimer = setTimeout(
    () => {
      expiryTimer = null;
      emit();
    },
    Math.max(0, ts - Date.now()),
  );
  emit();
}

function emit(): void {
  for (const l of listeners) l();
}

export function subscribeMainDegraded(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}
