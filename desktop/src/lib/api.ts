import { fetch } from '@tauri-apps/plugin-http';
import { toast } from 'sonner';
import { useAppStatusStore } from '../stores/app-status';
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
import { isSoundCloudAppBan, showSoundCloudAppBanToast } from './soundcloud-ban-toast';
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
    if (isSoundCloudAppBan(res.status, body)) {
      showSoundCloudAppBanToast();
      useAppStatusStore.getState().setSoundcloudBlocked(true);
    } else if (res.status >= 500) {
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
  useAppStatusStore.getState().setSoundcloudBlocked(false);
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

function buildStreamUrl(base: string, trackUrn: string, premium: boolean, hq: boolean) {
  const params = new URLSearchParams();
  if (hq) params.set('hq', 'true');
  if (sessionId) params.set('session_id', sessionId);
  const path = premium ? '/premium' : '';
  return `${base}/stream/${encodeURIComponent(trackUrn)}${path}?${params.toString()}`;
}

export function streamUrl(trackUrn: string, hq = useSettingsStore.getState().highQualityStreaming) {
  const isPremium = getIsPremium();
  if (isPremium) {
    return buildStreamUrl(getStreamingPremiumBase(), trackUrn, true, hq);
  }
  return buildStreamUrl(getStreamingBase(), trackUrn, false, hq);
}

/**
 * Premium fallback chain:
 * 1. premium host + /premium endpoint
 * 2. premium host + standard endpoint
 * 3. standard host + /premium endpoint
 * 4. standard host + standard endpoint
 *
 * Non-premium: just standard host + standard endpoint.
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
  return [buildStreamUrl(sBase, trackUrn, false, hq)];
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
