import { fetch } from '@tauri-apps/plugin-http';
import type { Track } from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import { ApiError, getSessionId } from './api-client';
import {
  BYPASS_STORAGE_BASE,
  BYPASS_STREAMING_BASE,
  BYPASS_STREAMING_PREMIUM_BASE,
  STORAGE_BASE,
  STREAMING_BASE,
  STREAMING_PREMIUM_BASE,
} from './constants';
import { logHttpError, logHttpFailure, trackAsync } from './diagnostics';
import { isHealthy, markHealthy, markUnhealthy } from './host-health';
import { getIsPremium } from './premium-cache';

// ─── Types ──────────────────────────────────────────────────

export type ResolvedStreamingTrack = Partial<Track> & {
  full_duration?: number;
};

// ─── Host resolution ────────────────────────────────────────

function resolveStreamingBases(): string[] {
  const bypass = useSettingsStore.getState().bypassWhitelist;
  const premium = getIsPremium();

  if (bypass && premium) {
    // All 4 bases, healthy first
    const all = [
      BYPASS_STREAMING_PREMIUM_BASE,
      BYPASS_STREAMING_BASE,
      STREAMING_PREMIUM_BASE,
      STREAMING_BASE,
    ];
    const unique = [...new Set(all)];
    return unique.sort((a, b) => {
      const aH = isHealthy(a) ? 0 : 1;
      const bH = isHealthy(b) ? 0 : 1;
      return aH - bH;
    });
  }

  return [...new Set([STREAMING_PREMIUM_BASE, STREAMING_BASE])];
}

// ─── Streaming JSON ─────────────────────────────────────────

async function streamingJson<T = unknown>(path: string): Promise<T> {
  let lastError: unknown = null;

  const label = `GET ${path}`;

  for (const base of resolveStreamingBases()) {
    const url = `${base}${path}`;
    try {
      const res = await trackAsync(`streaming:${label}`, fetch(url));

      if (!res.ok) {
        const body = await res.text();
        logHttpError(`streaming:${label}`, res.status, url, body);
        markUnhealthy(base);
        lastError = new ApiError(res.status, body);
        continue;
      }

      markHealthy(base);

      const contentType = res.headers.get('content-type');
      if (!contentType?.includes('application/json')) {
        throw new Error(`Unexpected content-type: ${contentType ?? 'unknown'}`);
      }

      return res.json();
    } catch (error) {
      if (error instanceof ApiError) {
        lastError = error;
        continue;
      }
      logHttpFailure(`streaming:${label}`, url, error);
      markUnhealthy(base);
      lastError = error;
    }
  }

  throw lastError ?? new Error('Streaming request failed');
}

// ─── Public API ─────────────────────────────────────────────

export function resolveTrackFromStreaming(url: string) {
  return streamingJson<ResolvedStreamingTrack>(`/resolve?url=${encodeURIComponent(url)}`);
}

function buildStreamUrl(base: string, trackUrn: string, premium: boolean, hq: boolean) {
  const params = new URLSearchParams();
  if (hq) params.set('hq', 'true');
  const sid = getSessionId();
  if (sid) params.set('session_id', sid);
  const path = premium ? '/premium' : '';
  return `${base}/stream/${encodeURIComponent(trackUrn)}${path}?${params.toString()}`;
}

export function buildStorageUrls(
  trackUrn: string,
  hq = useSettingsStore.getState().highQualityStreaming,
): string[] {
  const file = `${trackUrn.replace(/:/g, '_')}.ogg`;
  const qualities = hq ? ['hq', 'sq'] : ['sq', 'hq'];
  const bypass = useSettingsStore.getState().bypassWhitelist;
  const bases = bypass && getIsPremium() ? [BYPASS_STORAGE_BASE, STORAGE_BASE] : [STORAGE_BASE];
  return [...new Set(bases)].flatMap((base) => qualities.map((q) => `${base}/${q}/${file}`));
}

export function streamFallbackUrls(
  trackUrn: string,
  hq = useSettingsStore.getState().highQualityStreaming,
): string[] {
  const bases = resolveStreamingBases();
  const urls: string[] = [];
  const seen = new Set<string>();

  for (const base of bases) {
    // premium endpoint first, then standard
    for (const premium of [true, false]) {
      const url = buildStreamUrl(base, trackUrn, premium, hq);
      if (!seen.has(url)) {
        seen.add(url);
        urls.push(url);
      }
    }
  }

  return urls;
}
