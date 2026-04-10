import { fetch } from '@tauri-apps/plugin-http';
import { toast } from 'sonner';
import { useAppStatusStore } from '../stores/app-status';
import { useSettingsStore } from '../stores/settings';
import { API_BASE, STREAMING_BASE, STREAMING_PREMIUM_BASE } from './constants';
import { trackAsync } from './diagnostics';
import { isSoundCloudAppBan, showSoundCloudAppBanToast } from './soundcloud-ban-toast';
import { getIsPremium } from './subscription';

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
      fetch(`${API_BASE}${path}`, {
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
    // Premium users get premium host + premium endpoint as primary
    return buildStreamUrl(STREAMING_PREMIUM_BASE, trackUrn, true, hq);
  }
  return buildStreamUrl(STREAMING_BASE, trackUrn, false, hq);
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
  if (isPremium) {
    return [
      buildStreamUrl(STREAMING_PREMIUM_BASE, trackUrn, true, hq),
      buildStreamUrl(STREAMING_PREMIUM_BASE, trackUrn, false, hq),
      buildStreamUrl(STREAMING_BASE, trackUrn, true, hq),
      buildStreamUrl(STREAMING_BASE, trackUrn, false, hq),
    ];
  }
  return [buildStreamUrl(STREAMING_BASE, trackUrn, false, hq)];
}
