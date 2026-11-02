// The single entry point for the frontend's requests to our domains: it walks the tiers
// (direct → relay) and remembers what worked.

import { fetch } from '@tauri-apps/plugin-http';
import { type Hop, noteHop, planHops } from './config';

export type { Tier } from './config';
export { initEdge, tierOf } from './config';

function timedFetch(url: string, init: RequestInit, timeoutMs?: number): Promise<Response> {
  if (!timeoutMs || init.signal) return fetch(url, init) as Promise<Response>;
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  return fetch(url, { ...init, signal: controller.signal }).finally(() =>
    clearTimeout(timer),
  ) as Promise<Response>;
}

/**
 * An answer arrived — but is the transport to blame? Relay serves its own 421/502/503/504,
 * the direct host serves the balancer's HTML page. Both are cured by the next hop,
 * while the origin's answer (the app's 401/404/500) is a valid result already.
 */
function hopUsable(hop: Hop, res: Response): boolean {
  if (hop.tier === 'relay') return ![421, 502, 503, 504].includes(res.status);
  return !isDirectInfrastructureError(res);
}

function isDirectInfrastructureError(res: Response): boolean {
  if (res.status < 502 || res.status > 504) return false;
  return res.headers.get('content-type')?.toLowerCase().includes('text/html') ?? false;
}

/**
 * A short budget (host probe — 3 s) does not prove the direct path is closed:
 * a first cold TLS does not fit into it on a slow link. Such a request still
 * moves on to the next tier, but we do not move the verdict — otherwise healthy users
 * migrate to relay because of one slow probe. A real ban cuts the connection
 * immediately (RST), so this does not affect detection speed.
 */
const WEAK_BUDGET_MS = 5_000;

function isTimeout(error: unknown): boolean {
  if (error instanceof DOMException && error.name === 'AbortError') return true;
  const msg = error instanceof Error ? error.message.toLowerCase() : '';
  return msg.includes('abort') || msg.includes('timeout') || msg.includes('timed out');
}

/**
 * A TRANSPORT refusal that reached the caller: a CF/balancer page instead of an
 * app answer. A separate type so `api-client` does not take it for an API response —
 * otherwise a worker's 429 reads as «we are being rate-limited», a balancer's 502 as
 * «the server is down», and both wake session recovery for nothing.
 */
export class EdgeTransportError extends Error {
  constructor(
    readonly tier: string,
    readonly status: number,
  ) {
    super(`edge: ${tier} answered ${status} (transport, not the application)`);
    this.name = 'EdgeTransportError';
  }
}

/**
 * Reserve for the spare hops. The budget cannot be split evenly: usually the FIRST
 * hop answers, and its legitimately slow answer is worth more than the chance to try the
 * reserve. With a 30 s budget the direct path gets 20 s — enough for the 17.9 s
 * observed in prod under the refresh-lock convoy, and relay still keeps 10 s.
 */
const FALLBACK_RESERVE_MS = 10_000;

/** How much time to give this hop; the last one gets the whole remainder */
function hopBudgetMs(remaining: number, hopsLeft: number): number {
  if (hopsLeft <= 1) return remaining;
  const reserve = Math.min(FALLBACK_RESERVE_MS, Math.floor(remaining / 3));
  return remaining - reserve;
}

export async function edgeFetch(
  url: string,
  init: RequestInit = {},
  timeoutMs?: number,
): Promise<Response> {
  const hops = planHops(url);
  if (hops.length === 0) return timedFetch(url, init, timeoutMs);

  const weakBudget = timeoutMs !== undefined && timeoutMs < WEAK_BUDGET_MS;
  // The budget covers the WHOLE call, not each hop. Otherwise the timeout silently multiplies
  // by the plan length: 10 s of control-plane became 20 s over two hops and
  // 40 s over two bases, and the caller got «Request canceled» instead of an answer.
  const deadline = timeoutMs === undefined ? undefined : Date.now() + timeoutMs;
  let lastError: unknown = null;

  for (let i = 0; i < hops.length; i++) {
    const hop = hops[i];
    const isLast = i === hops.length - 1;
    const remaining = deadline === undefined ? undefined : deadline - Date.now();
    if (remaining !== undefined && remaining <= 0) break;
    const hopBudget = remaining === undefined ? undefined : hopBudgetMs(remaining, hops.length - i);

    try {
      const res = await timedFetch(hop.url, init, hopBudget);
      if (hopUsable(hop, res)) {
        noteHop(hop, true);
        return res;
      }
      if (hop.tier === 'direct') noteHop(hop, false);
      if (isLast) throw new EdgeTransportError(hop.tier, res.status);
    } catch (error) {
      if (error instanceof EdgeTransportError) throw error;
      lastError = error;
      if (!(weakBudget && isTimeout(error))) noteHop(hop, false);
      // Aborted by the caller (not a hop timeout) — further attempts are pointless.
      if (init.signal?.aborted) throw error;
      if (isLast) throw error;
    }
  }
  throw lastError ?? new Error('edge: budget exhausted before any hop answered');
}
