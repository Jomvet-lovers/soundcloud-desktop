import { fetch } from '@tauri-apps/plugin-http';
import { useAppStatusStore } from '../../stores/app-status';
import { API_BASE, API_STAR_BASE } from '../constants';
import { edgeFetch } from '../edge';
import { requestPremiumRecheck } from '../premium-cache';
import { queryClient } from '../query-client';
import { type NetVerdict, useHostStatusStore } from './store';

// ─── Health map (per-request data-plane routing) ────────────

const UNHEALTHY_DURATION_MS = 30_000;
const unhealthyUntil = new Map<string, number>();

export function isHealthy(host: string): boolean {
  const until = unhealthyUntil.get(host);
  if (until === undefined) return true;
  if (Date.now() > until) {
    unhealthyUntil.delete(host);
    return true;
  }
  return false;
}

export function markHealthy(host: string): void {
  unhealthyUntil.delete(host);
  if (host === API_BASE) noteMainAlive();
  else if (host === API_STAR_BASE && useHostStatusStore.getState().star !== 'up') {
    useHostStatusStore.setState({ star: 'up' });
  }
}

/** A passive failure does NOT change the verdict — it sets a cooldown and schedules a main probe */
export function markUnhealthy(host: string): void {
  unhealthyUntil.set(host, Date.now() + UNHEALTHY_DURATION_MS);
  if (host === API_BASE) requestProbe();
}

/** Any main success (real request or probe). Hot path: no-op when already up */
export function noteMainAlive(): void {
  mainAliveGen++;
  const prev = useHostStatusStore.getState().main;
  if (prev === 'up') return;
  useHostStatusStore.setState({ main: 'up', net: 'online' });
  useAppStatusStore.getState().setBackendReachable(true);
  stopRecheckTimer();
  if (prev === 'down') void queryClient.invalidateQueries();
}

// ─── Probe engine ───────────────────────────────────────────

const PROBE_TIMEOUT_MS = 3_000;
const CONFIRM_DELAY_MS = 2_000;
const PROBE_MIN_GAP_MS = 5_000;
const RECHECK_MS = 15_000;
const MODAL_RESHOW_SUPPRESS_MS = 10 * 60_000;
const TIMEOUT_BURST_WINDOW_MS = 30_000;
/** Timeout threshold per window: below it are isolated slow requests, not a host down */
const TIMEOUT_BURST_THRESHOLD = 3;

interface ProbeResult {
  alive: boolean;
  netFail: boolean;
}

/** Timeout / abort by timeout: our AbortController or a reqwest timeout/connect error */
export function isTimeoutError(error: unknown): boolean {
  if (error instanceof DOMException && error.name === 'AbortError') return true;
  const msg = error instanceof Error ? error.message : typeof error === 'string' ? error : '';
  const m = msg.toLowerCase();
  return (
    m.includes('abort') ||
    m.includes('cancel') ||
    m.includes('timeout') ||
    m.includes('timed out') ||
    m.includes('time out')
  );
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/** Our hosts go through the edge tiers (otherwise a banned user's probe sees «everything down») */
async function fetchWithAbort(url: string): Promise<Response> {
  return edgeFetch(url, { cache: 'no-store' as RequestCache }, PROBE_TIMEOUT_MS);
}

/** External internet beacons — strictly direct, the tiers do not apply here */
async function fetchExternal(url: string): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    return await fetch(url, { cache: 'no-store', signal: controller.signal });
  } finally {
    clearTimeout(timer);
  }
}

/** Status <500 = host alive (401/403/429 are answers too); network/timeout = netFail */
async function probeOnce(base: string): Promise<ProbeResult> {
  try {
    const res = await fetchWithAbort(`${base}/health`);
    return { alive: res.status < 500, netFail: false };
  } catch {
    return { alive: false, netFail: true };
  }
}

/** up — from a single success; down — only from two failures with a pause (anti-flap) */
async function probeConfirmed(base: string): Promise<ProbeResult> {
  const first = await probeOnce(base);
  if (first.alive) return first;
  await sleep(CONFIRM_DELAY_MS);
  const second = await probeOnce(base);
  if (second.alive) return second;
  return { alive: false, netFail: first.netFail && second.netFail };
}

async function validatedFetch(
  url: string,
  valid: (res: Response) => boolean | Promise<boolean>,
): Promise<boolean> {
  try {
    return await valid(await fetchExternal(url));
  } catch {
    return false;
  }
}

function anyTrue(checks: Promise<boolean>[]): Promise<boolean> {
  return new Promise((resolve) => {
    let pending = checks.length;
    for (const check of checks) {
      void check.then((ok) => {
        if (ok) resolve(true);
        else if (--pending === 0) resolve(false);
      });
    }
  });
}

/** Only on a mutual network failure of main+star; strict validation against captive portals */
async function checkInternet(): Promise<NetVerdict> {
  const online = await anyTrue([
    validatedFetch('https://www.gstatic.com/generate_204', (res) => res.status === 204),
    validatedFetch(
      'https://detectportal.firefox.com/success.txt',
      async (res) => res.status === 200 && (await res.text()).startsWith('success'),
    ),
    validatedFetch('https://www.cloudflare.com/cdn-cgi/trace', (res) => res.status === 200),
  ]);
  return online ? 'online' : 'no-internet';
}

let lastRunAt = 0;
// Freshness: a real main success that happens during run() beats the probe verdict.
let mainAliveGen = 0;
let trailingTimer: ReturnType<typeof setTimeout> | null = null;
let recheckTimer: ReturnType<typeof setInterval> | null = null;
let timeoutHits: number[] = [];

function timeoutBurst(): boolean {
  const now = Date.now();
  timeoutHits = timeoutHits.filter((t) => now - t < TIMEOUT_BURST_WINDOW_MS);
  return timeoutHits.length >= TIMEOUT_BURST_THRESHOLD;
}

/** A real request timed out. Force a probe only when all requests time out (burst), not 1-2 slow ones */
export function noteRequestTimeout(): void {
  timeoutHits.push(Date.now());
  if (timeoutBurst()) requestProbe({ force: true });
}

function startRecheckTimer(): void {
  recheckTimer ??= setInterval(() => requestProbe(), RECHECK_MS);
}

function stopRecheckTimer(): void {
  if (recheckTimer !== null) {
    clearInterval(recheckTimer);
    recheckTimer = null;
  }
}

/** Single-flight + min-gap with a trailing catch-up; force skips the min-gap, not the single-flight */
export function requestProbe(opts?: { force?: boolean }): void {
  if (!navigator.onLine || useHostStatusStore.getState().probing) return;
  const sinceLast = Date.now() - lastRunAt;
  if (sinceLast < PROBE_MIN_GAP_MS && !opts?.force) {
    if (trailingTimer === null) {
      trailingTimer = setTimeout(() => {
        trailingTimer = null;
        requestProbe();
      }, PROBE_MIN_GAP_MS - sinceLast);
    }
    return;
  }
  useHostStatusStore.setState({ probing: true });
  lastRunAt = Date.now();
  void run().finally(() => {
    useHostStatusStore.setState({ probing: false });
  });
}

async function run(): Promise<void> {
  const main = await probeConfirmed(API_BASE);
  if (main.alive) {
    useHostStatusStore.setState({ star: 'unknown', net: 'online' });
    markHealthy(API_BASE); // noteMainAlive: up + stops the recheck timer (no-op when already up)
    // Clear a false offline even when the verdict is already 'up' (noteMainAlive is a no-op then).
    useAppStatusStore.getState().setBackendReachable(true);
    return;
  }
  const genAfterMainProbes = mainAliveGen;
  const star = await probeConfirmed(API_STAR_BASE);
  // A timeout burst = every request times out → the host is down, not offline: do not mute the modal.
  if (
    main.netFail &&
    star.netFail &&
    !timeoutBurst() &&
    (await checkInternet()) === 'no-internet'
  ) {
    // We do not know whether the hosts are down; leave backendReachable alone — apiRequest drives the offline flow.
    useHostStatusStore.setState({ main: 'unknown', star: 'unknown', net: 'no-internet' });
    startRecheckTimer();
    return;
  }
  // A real main success during the star probe/internet check — the result is stale, do not record down.
  if (mainAliveGen !== genAfterMainProbes) return;
  const prev = useHostStatusStore.getState();
  const newIncident = prev.main !== 'down';
  const incidentId = newIncident ? prev.incidentId + 1 : prev.incidentId;
  useHostStatusStore.setState({
    main: 'down',
    star: star.alive ? 'up' : 'down',
    net: 'online',
    incidentId,
    // Flap guard: a modal closed recently does not come back on a new incident.
    ...(newIncident && Date.now() - prev.lastModalDismissAt < MODAL_RESHOW_SUPPRESS_MS
      ? { modalDismissedIncidentId: incidentId }
      : {}),
  });
  if (star.alive) markHealthy(API_STAR_BASE); // map + verdict; backendReachable untouched
  requestPremiumRecheck(); // stale premium=true / premium lost by a subscriber
  startRecheckTimer();
}

let initialized = false;

/** Boot probe + wake triggers (network came back / laptop woke). Idempotent */
export function initHostStatus(): void {
  if (initialized) return;
  initialized = true;
  requestProbe();
  const onWake = () => {
    if (useHostStatusStore.getState().main !== 'up') requestProbe();
  };
  window.addEventListener('online', onWake);
  window.addEventListener('focus', onWake);
}
