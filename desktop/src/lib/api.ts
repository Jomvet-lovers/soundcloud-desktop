import { fetch } from '@tauri-apps/plugin-http';
import { toast } from 'sonner';
import { useAppStatusStore } from '../stores/app-status';
import type { Track } from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import {
  API_BASE,
  BYPASS_API_BASE,
  BYPASS_STREAMING_BASE,
  BYPASS_STREAMING_PREMIUM_BASE,
  STREAMING_BASE,
  STREAMING_PREMIUM_BASE,
} from './constants';
import { trackAsync } from './diagnostics';
import { getIsPremium } from './subscription';

export function getApiBase() {
  return useSettingsStore.getState().bypassWhitelist ? BYPASS_API_BASE : API_BASE;
}

function getStreamingBase() {
  return useSettingsStore.getState().bypassWhitelist ? BYPASS_STREAMING_BASE : STREAMING_BASE;
}

function getStreamingPremiumBase() {
  return useSettingsStore.getState().bypassWhitelist
    ? BYPASS_STREAMING_PREMIUM_BASE
    : STREAMING_PREMIUM_BASE;
}

let sessionId: string | null = null;

export function setSessionId(id: string | null) {
  sessionId = id;
}

export function getSessionId() {
  return sessionId;
}

export async function api<T = unknown>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (sessionId) {
    headers.set('x-session-id', sessionId);
  }
  if (!headers.has('Content-Type') && options.body) {
    headers.set('Content-Type', 'application/json');
  }

  const method = options.method ?? 'GET';
  let res: Awaited<ReturnType<typeof fetch>>;
  try {
    res = await trackAsync(
      `http:${method.toUpperCase()} ${path}`,
      fetch(`${getApiBase()}${path}`, {
        ...options,
        headers,
      }),
    );
    useAppStatusStore.getState().setBackendReachable(true);
  } catch (error) {
    useAppStatusStore.getState().setBackendReachable(false);
    throw error;
  }

  if (!res.ok) {
    const body = await res.text();
    const err = new ApiError(res.status, body);
    if (res.status >= 500) {
      toast.error(`Server error (${res.status})`);
    } else if (res.status === 401) {
      toast.error('Session expired');
    } else if (res.status >= 400) {
      try {
        const parsed = JSON.parse(body);
        toast.error(parsed.message || parsed.error || `Error ${res.status}`);
      } catch {
        toast.error(`Error ${res.status}`);
      }
    }
    console.error(`HTTP ERROR: url: ${path}, `, err);
    throw err;
  }

  const contentType = res.headers.get('content-type');
  if (contentType?.includes('application/json')) {
    return res.json();
  }
  return res.text() as T;
}

export class ApiError extends Error {
  constructor(
    public status: number,
    public body: string,
  ) {
    super(`API ${status}: ${body}`);
    this.name = 'ApiError';
  }
}

export type ResolvedStreamingTrack = Partial<Track> & {
  full_duration?: number;
};

function getStreamingBases() {
  return [...new Set([getStreamingBase(), getStreamingPremiumBase()])];
}

async function streamingJson<T = unknown>(path: string): Promise<T> {
  let lastError: unknown = null;

  for (const base of getStreamingBases()) {
    try {
      const res = await trackAsync(`streaming:GET ${path}`, fetch(`${base}${path}`));

      if (!res.ok) {
        const body = await res.text();
        throw new ApiError(res.status, body);
      }

      const contentType = res.headers.get('content-type');
      if (!contentType?.includes('application/json')) {
        throw new Error(`Unexpected content-type: ${contentType ?? 'unknown'}`);
      }

      return res.json();
    } catch (error) {
      lastError = error;
    }
  }

  throw lastError ?? new Error('Streaming request failed');
}

export function resolveTrackFromStreaming(url: string) {
  return streamingJson<ResolvedStreamingTrack>(`/resolve?url=${encodeURIComponent(url)}`);
}

function buildStreamUrl(base: string, trackUrn: string, premium: boolean, hq: boolean) {
  const params = new URLSearchParams();
  if (hq) params.set('hq', 'true');
  if (sessionId) params.set('session_id', sessionId);
  const path = premium ? '/premium' : '';
  return `${base}/stream/${encodeURIComponent(trackUrn)}${path}?${params.toString()}`;
}

/**
 * Premium fallback chain:
 * 1. premium host + /premium endpoint
 * 2. premium host + standard endpoint
 * 3. standard host + /premium endpoint
 * 4. standard host + standard endpoint
 *
 * Non-premium (or subscription not yet loaded):
 * 1. standard host + /premium endpoint (backend checks subscription)
 * 2. standard host + standard endpoint
 */
export function streamFallbackUrls(
  trackUrn: string,
  hq = useSettingsStore.getState().highQualityStreaming,
): string[] {
  const isPremium = getIsPremium();
  const sBase = getStreamingBase();
  const spBase = getStreamingPremiumBase();
  if (isPremium) {
    return [
      buildStreamUrl(spBase, trackUrn, true, hq),
      buildStreamUrl(spBase, trackUrn, false, hq),
      buildStreamUrl(sBase, trackUrn, true, hq),
      buildStreamUrl(sBase, trackUrn, false, hq),
    ];
  }
  return [buildStreamUrl(sBase, trackUrn, true, hq), buildStreamUrl(sBase, trackUrn, false, hq)];
}

/**
 * Auth-specific fetch with fallback: try primary API (10s), then bypass API (10s), then error.
 */
export async function fetchWithAuthFallback<T = unknown>(
  path: string,
  options: RequestInit = {},
): Promise<T> {
  const headers = new Headers(options.headers);
  if (sessionId) headers.set('x-session-id', sessionId);
  if (!headers.has('Content-Type') && options.body) headers.set('Content-Type', 'application/json');

  const doFetch = (base: string, timeoutMs: number) => {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    return fetch(`${base}${path}`, { ...options, headers, signal: controller.signal }).finally(() =>
      clearTimeout(timer),
    );
  };

  // 1) Try primary API
  try {
    const res = await doFetch(API_BASE, 10_000);
    useAppStatusStore.getState().setBackendReachable(true);
    if (!res.ok) {
      const body = await res.text();
      throw new ApiError(res.status, body);
    }
    const ct = res.headers.get('content-type');
    return ct?.includes('application/json') ? res.json() : (res.text() as T);
  } catch {
    // primary failed / timed out — try bypass
  }

  // 2) Try bypass API
  try {
    const res = await doFetch(BYPASS_API_BASE, 10_000);
    useAppStatusStore.getState().setBackendReachable(true);
    if (!res.ok) {
      const body = await res.text();
      throw new ApiError(res.status, body);
    }
    const ct = res.headers.get('content-type');
    return ct?.includes('application/json') ? res.json() : (res.text() as T);
  } catch (err) {
    useAppStatusStore.getState().setBackendReachable(false);
    throw err instanceof ApiError ? err : new Error('Connection failed');
  }
}
