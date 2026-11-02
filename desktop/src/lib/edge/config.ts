// Transport tiers to our domains: direct host → relay pool
// (`<service>.<node>.relay.scnative.space`). The CF-workers tier is gone —
// its limits did not hold our traffic, see `network/edge.rs`.
// The table and persistence live in Rust (`network/edge.rs`); this is the mirror of the verdict for
// the requests the frontend makes itself. The policy is the same, so both worlds converge on
// one tier; only the verdict CHANGE goes to the core, not every request.
// The relay pool roster arrives from the core — a new node does not have to be written here.

import { trackedInvoke as invoke } from '../diagnostics';

export type Tier = 'direct' | 'relay';

export interface Hop {
  url: string;
  tier: Tier;
  origin: string;
}

interface RustConfig {
  relays: [string, string[]][];
  hints: Record<string, Tier>;
  revalidate_ms: number;
}

interface OriginState {
  tier: Tier;
  revalidateAt: number;
  directFails: number;
}

const TIER_ORDER: Record<Tier, number> = { direct: 0, relay: 1 };
const DIRECT_FAIL_THRESHOLD = 2;

let relays = new Map<string, string[]>();
let revalidateMs = 600_000;
const origins = new Map<string, OriginState>();

/** Call before the first network request. Without config the direct path stays */
export async function initEdge(): Promise<void> {
  try {
    const cfg = await invoke<RustConfig>('edge_config');
    relays = new Map(cfg.relays);
    revalidateMs = cfg.revalidate_ms || revalidateMs;
    const now = Date.now();
    for (const [host, tier] of Object.entries(cfg.hints ?? {})) {
      if (tier === 'direct') continue;
      origins.set(host, {
        tier,
        revalidateAt: now + revalidateMs,
        directFails: DIRECT_FAIL_THRESHOLD,
      });
    }
  } catch {
    // The core did not answer — keep going as before.
  }
}

function hostOf(url: string): string | null {
  try {
    return new URL(url).hostname.toLowerCase();
  } catch {
    return null;
  }
}

function swapHost(url: string, host: string): string {
  try {
    const u = new URL(url);
    u.protocol = 'https:';
    u.hostname = host;
    u.port = '';
    return u.toString();
  } catch {
    return url;
  }
}

/** Empty = not our domain, go as is */
export function planHops(url: string): Hop[] {
  const origin = hostOf(url);
  if (!origin) return [];
  const pool = relays.get(origin);
  if (!pool?.length) return [];

  const now = Date.now();
  const state = origins.get(origin);
  // Until revalidation, do not try tiers below the stuck one — otherwise every request
  // pays a timeout for a path that is known to be closed.
  const from = !state || now >= state.revalidateAt ? 'direct' : state.tier;
  const min = TIER_ORDER[from];

  const hops: Hop[] = [];
  if (min <= TIER_ORDER.direct) hops.push({ url, tier: 'direct', origin });
  if (min <= TIER_ORDER.relay) {
    for (const relay of pool) hops.push({ url: swapHost(url, relay), tier: 'relay', origin });
  }

  if (min > TIER_ORDER.direct) {
    // The direct path stays as the last chance even when the verdict moved to relay:
    // the relay node may go down, and by then the origin may be unblocked.
    hops.push({ url, tier: 'direct', origin });
  }
  return hops;
}

/**
 * What goes to the core is the CONCLUSION (the current tier), not an event: Rust keeps its own
 * failure counter, and a lone «direct did not answer» would not move it — the frontend verdict
 * would be lost. Only the change is sent, so IPC does not run on every request.
 */
const reported = new Map<string, Tier>();

function report(origin: string, tier: Tier): void {
  if (reported.get(origin) === tier) return;
  reported.set(origin, tier);
  void invoke('edge_note', { origin, tier, ok: true }).catch(() => {});
}

export function noteHop(hop: Hop, ok: boolean): void {
  const now = Date.now();
  const prev = origins.get(hop.origin);

  if (ok) {
    const tier = hop.tier;
    // Only a TIER CHANGE restarts the revalidation clock — otherwise under live
    // traffic it never counts down and the user stays on relay forever.
    const revalidateAt = prev && prev.tier === tier ? prev.revalidateAt : now + revalidateMs;
    origins.set(hop.origin, {
      tier,
      revalidateAt,
      directFails: tier === 'direct' ? 0 : DIRECT_FAIL_THRESHOLD,
    });
    report(hop.origin, tier);
    return;
  }

  if (hop.tier !== 'direct') return;
  const state = prev ?? { tier: 'direct' as Tier, revalidateAt: now, directFails: 0 };
  state.directFails += 1;
  state.revalidateAt = now + revalidateMs;
  if (state.tier === 'direct' && state.directFails >= DIRECT_FAIL_THRESHOLD) state.tier = 'relay';
  origins.set(hop.origin, state);
  if (state.tier !== 'direct') report(hop.origin, state.tier);
}

/** Current tier — for diagnostics and the status banner */
export function tierOf(origin: string): Tier {
  return origins.get(origin)?.tier ?? 'direct';
}
