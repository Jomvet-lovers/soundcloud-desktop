import type { Track } from '../stores/player';
import { useSettingsStore } from '../stores/settings';
import { ApiError, getSessionId } from './api-client';
import {
  STORAGE_BASE,
  STORAGE_PREMIUM_BASE,
  STREAMING_BASE,
  STREAMING_PREMIUM_BASE,
} from './constants';
import { logHttpError, logHttpFailure, trackAsync } from './diagnostics';
import { edgeFetch } from './edge';
import { markHealthy, markUnhealthy } from './host-status';
// Straight from premium-cache, not subscription: that one pulls in api-client and
// query-client, and importing from here would close the cycle (see premium-cache's header).
import { getIsPremium } from './premium-cache';

// ─── Types ──────────────────────────────────────────────────

export type ResolvedStreamingTrack = Partial<Track> & {
  full_duration?: number;
  /// SC resolve returns any entity: track / user / playlist (albums included).
  kind?: string;
};

// ─── Host resolution ────────────────────────────────────────

function resolveStreamingBases(): string[] {
  return [...new Set([STREAMING_PREMIUM_BASE, STREAMING_BASE])];
}

// ─── Streaming JSON ─────────────────────────────────────────

async function streamingJson<T = unknown>(path: string): Promise<T> {
  let lastError: unknown = null;

  const label = `GET ${path}`;

  for (const base of resolveStreamingBases()) {
    const url = `${base}${path}`;
    try {
      const res = await trackAsync(`streaming:${label}`, edgeFetch(url));

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

/// SC resolve returns a track / user / playlist. Playlists (albums included)
/// arrive without `urn`, so rebuild one from kind + id. Null when the entity has
/// no page in this app.
const KIND_ROUTES: Record<string, { route: string; ns: string }> = {
  track: { route: 'track', ns: 'tracks' },
  user: { route: 'user', ns: 'users' },
  playlist: { route: 'playlist', ns: 'playlists' },
};

export function resolvedRoute(resolved: ResolvedStreamingTrack | null | undefined): string | null {
  if (!resolved) return null;
  const target = KIND_ROUTES[resolved.kind ?? 'track'];
  if (!target) return null;
  const urn =
    resolved.urn ?? (resolved.id != null ? `soundcloud:${target.ns}:${resolved.id}` : null);
  return urn ? `/${target.route}/${encodeURIComponent(urn)}` : null;
}

function buildStreamUrl(base: string, trackUrn: string, hq: boolean) {
  const params = new URLSearchParams();
  if (hq) params.set('hq', 'true');
  const sid = getSessionId();
  if (sid) params.set('session_id', sid);
  return `${base}/stream/${encodeURIComponent(trackUrn)}?${params.toString()}`;
}

/**
 * Media-serving endpoints, preferred first. Premium pulls bytes from the reserve
 * one (read-only S3 on star-host) instead of chasing every file on main; the core
 * orders the list by host health on its own and walks it until the first answer.
 *
 * To a non-premium user star returns 403 (`PREMIUM_ONLY`), so a second address
 * would only cost an extra request — their list is a single main.
 */
export function buildStorageUrls(trackUrn: string): string[] {
  const file = `${trackUrn.replace(/:/g, '_')}.m4a`;
  const bases = getIsPremium() ? [STORAGE_PREMIUM_BASE, STORAGE_BASE] : [STORAGE_BASE];
  return [...new Set(bases)].map((base) => `${base}/${file}`);
}

export function streamFallbackUrls(
  trackUrn: string,
  hq = useSettingsStore.getState().highQualityStreaming,
): string[] {
  const bases = resolveStreamingBases();
  const urls: string[] = [];
  const seen = new Set<string>();

  for (const base of bases) {
    const url = buildStreamUrl(base, trackUrn, hq);
    if (!seen.has(url)) {
      seen.add(url);
      urls.push(url);
    }
  }

  return urls;
}

function buildDownloadUrl(base: string, trackUrn: string, hq: boolean) {
  const params = new URLSearchParams();
  if (hq) params.set('hq', 'true');
  const sid = getSessionId();
  if (sid) params.set('session_id', sid);
  const qs = params.toString();
  const suffix = qs ? `?${qs}` : '';
  return `${base}/download/${encodeURIComponent(trackUrn)}${suffix}`;
}

/// `/download/:urn` URLs across all valid streaming bases.
/// The client hits them between the anon and storage stream: the server only resolves
/// SoundCloud links + (for encrypted) does the Widevine handshake, segment download
/// goes straight to SC.
export function downloadFallbackUrls(
  trackUrn: string,
  hq = useSettingsStore.getState().highQualityStreaming,
): string[] {
  const bases = resolveStreamingBases();
  const urls: string[] = [];
  const seen = new Set<string>();

  for (const base of bases) {
    const url = buildDownloadUrl(base, trackUrn, hq);
    if (!seen.has(url)) {
      seen.add(url);
      urls.push(url);
    }
  }

  return urls;
}
