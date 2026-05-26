import { fetch } from '@tauri-apps/plugin-http';
import { toast } from 'sonner';
import { useAppStatusStore } from '../stores/app-status';
import { useAuthStore } from '../stores/auth';
import { noteAuthGap, noteRateLimit, noteSuccess } from './auth-recovery';
import { API_BASE } from './constants';
import { logHttpError, logHttpFailure, trackAsync } from './diagnostics';
import { markHealthy, markUnhealthy } from './host-health';

// ─── Session ────────────────────────────────────────────────

let sessionId: string | null = null;

export function setSessionId(id: string | null) {
  sessionId = id;
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

function isRateLimitError(status: number, body: string): boolean {
  if (status === 429) return true;
  const b = body.toLowerCase();
  return b.includes('rate limit') || b.includes('rate-limited') || b.includes('too many requests');
}

// ─── Helpers ────────────────────────────────────────────────

function fetchWithTimeout(
  url: string,
  options: RequestInit,
  timeoutMs: number = 60_000,
): Promise<Response> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  return fetch(url, { ...options, signal: controller.signal }).finally(() =>
    clearTimeout(timer),
  ) as Promise<Response>;
}

function handleApiError(err: ApiError): void {
  if (err.status >= 500) {
    toast.error(`Server error (${err.status})`);
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

export async function apiRequest<T = unknown>(
  path: string,
  options: RequestInit = {},
  timeoutMs?: number,
): Promise<T> {
  const headers = new Headers(options.headers);
  // Защита от попадания строки "undefined"/"null" в header при апгрейдах формата API.
  if (sessionId && sessionId !== 'undefined' && sessionId !== 'null') {
    headers.set('x-session-id', sessionId);
  }
  if (!headers.has('Content-Type') && options.body) headers.set('Content-Type', 'application/json');

  const method = options.method ?? 'GET';
  const label = `${method.toUpperCase()} ${path}`;
  const url = `${API_BASE}${path}`;
  const attemptStart = performance.now();

  try {
    const res = await trackAsync(
      `http:${label}`,
      fetchWithTimeout(url, { ...options, headers }, timeoutMs),
    );

    markHealthy(API_BASE);
    useAppStatusStore.getState().setBackendReachable(true);

    if (!res.ok) {
      const body = await res.text();
      const err = new ApiError(res.status, body);
      logHttpError(label, res.status, url, body);

      // Rate-limit — копим, одиночный не дёргает recovery.
      if (isRateLimitError(res.status, body)) {
        noteRateLimit();
        console.error(`HTTP ERROR: url: ${path}, `, err);
        throw err;
      }

      // Протухший токен (401) либо юзер пропал из сайдбара — сильный
      // сигнал, silent renew сразу.
      if (res.status === 401 || useAuthStore.getState().user == null) {
        noteAuthGap();
        console.error(`HTTP ERROR: url: ${path}, `, err);
        throw err;
      }

      handleApiError(err);
      console.error(`HTTP ERROR: url: ${path}, `, err);
      throw err;
    }

    // Успешный ответ — чистит rate-limit накопитель и само-гасит recovery,
    // если всё ожило само.
    noteSuccess();

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
    logHttpFailure(label, url, error, performance.now() - attemptStart);
    markUnhealthy(API_BASE);
    useAppStatusStore.getState().setBackendReachable(false);
    throw error;
  }
}

// ─── Aliases ────────────────────────────────────────────────

export const fetchWithAuthFallback = apiRequest;
