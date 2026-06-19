import { fetch } from '@tauri-apps/plugin-http';
import { toast } from 'sonner';
import i18n from '../i18n';
import { useAppStatusStore } from '../stores/app-status';
import { useAuthStore } from '../stores/auth';
import { noteAuthGap, noteRateLimit, noteSuccess } from './auth-recovery';
import { API_BASE, API_STAR_BASE } from './constants';
import { logHttpError, logHttpFailure, trackAsync } from './diagnostics';
import {
  getHostVerdict,
  isHealthy,
  isIncidentActive,
  isTimeoutError,
  markHealthy,
  markUnhealthy,
  noteRequestTimeout,
  preferredControlBase,
} from './host-status';
import { getIsPremium, requestPremiumRecheck } from './premium-cache';

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
    if (isIncidentActive()) return; // авария уже показана модалкой/баннером
    // Фиксированный id: sonner заменяет тост, шторм не стекается.
    toast.error(i18n.t('errors.serverError', { status: err.status }), { id: 'api-server-error' });
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

export type ApiRequestOptions = RequestInit & {
  /**
   * HTTP-статусы, которые считаем штатными: без error-тоста, без auth/rate-limit
   * recovery и без error-лога. ApiError всё равно бросается — тихо, чтобы вызвавший
   * мог свести его к дефолту (напр. 404 /related → пустой список похожих).
   */
  silentStatuses?: number[];
};

const CONTROL_PLANE_TIMEOUT_MS = 10_000;
const DOWN_HOST_TIMEOUT_MS = 10_000;

/**
 * Базы запроса, primary первой.
 * /me/subscription — открыт на star (bootstrap-сигнал премиума): перебор обоих, primary по вердикту main.
 * /auth/login* — прибит к main: OAuth redirect_uri зарегистрирован в SC только на основной хост.
 * остальной /auth/* (refresh, link/*, logout) — ровно ОДИН хост по вердикту: refresh нельзя
 *   фейловерить перебором (per-process refresh_locks на бэке → двойная ротация refresh_token),
 *   link-токены single-use.
 * data-plane — как раньше: премиум → star, GET/HEAD с фолбэком, мутации без (двойное применение хуже отказа).
 */
function apiBasesFor(path: string, method: string): string[] {
  if (path === '/me/subscription') {
    return getHostVerdict(API_BASE) === 'down'
      ? [API_STAR_BASE, API_BASE]
      : [API_BASE, API_STAR_BASE];
  }
  if (path.startsWith('/auth/login')) return [API_BASE];
  if (path.startsWith('/auth/')) return [preferredControlBase()];
  if (getIsPremium() && sessionId && isHealthy(API_STAR_BASE)) {
    const idempotent = method === 'GET' || method === 'HEAD';
    return idempotent ? [API_STAR_BASE, API_BASE] : [API_STAR_BASE];
  }
  return [API_BASE];
}

/** Host-фейл → фейловер. 4xx-контракты (400/404/…) — валидный ответ, не фейлим. */
function isHostFailover(status: number): boolean {
  return status >= 500 || status === 401 || status === 403;
}

export async function apiRequest<T = unknown>(
  path: string,
  options: ApiRequestOptions = {},
  timeoutMs?: number,
): Promise<T> {
  const { silentStatuses, ...init } = options;
  const headers = new Headers(init.headers);
  // Защита от попадания строки "undefined"/"null" в header при апгрейдах формата API.
  if (sessionId && sessionId !== 'undefined' && sessionId !== 'null') {
    headers.set('x-session-id', sessionId);
  }
  if (!headers.has('Content-Type') && init.body) headers.set('Content-Type', 'application/json');

  const method = init.method ?? 'GET';
  const label = `${method.toUpperCase()} ${path}`;
  const bases = apiBasesFor(path, method);
  // Control-plane короткий (login/refresh должны фейлиться быстро); 60 c дефолт
  // data-plane не трогаем — есть тяжёлые легитимные запросы.
  const isControlPlane = path === '/me/subscription' || path.startsWith('/auth/');
  const effectiveTimeout = timeoutMs ?? (isControlPlane ? CONTROL_PLANE_TIMEOUT_MS : undefined);
  let lastError: unknown = null;

  for (let i = 0; i < bases.length; i++) {
    const base = bases[i];
    const isLast = i === bases.length - 1;
    const url = `${base}${path}`;
    const attemptStart = performance.now();

    // Хост с вердиктом down не держит попытку дольше 10 c.
    const attemptTimeout =
      getHostVerdict(base) === 'down'
        ? Math.min(effectiveTimeout ?? 60_000, DOWN_HOST_TIMEOUT_MS)
        : effectiveTimeout;

    try {
      const res = await trackAsync(
        `http:${label}`,
        fetchWithTimeout(url, { ...init, headers }, attemptTimeout),
      );

      // Жив = ответил <500 (как probeOnce; 401/403 — валидный ответ axum, star они
      // НЕ марают — иначе протухший токен выключал бы star при мёртвом main).
      // ≥500 — пассивный фейл: cooldown + проба main; вердикт down ставит только проба.
      if (res.status < 500) markHealthy(base);
      else markUnhealthy(base);
      // Успех star для не-премиума — probe-сигнал, не «онлайн» (иначе флап offline↔online).
      if (base === API_BASE || getIsPremium()) {
        useAppStatusStore.getState().setBackendReachable(true);
      }

      if (!res.ok) {
        const body = await res.text();
        const err = new ApiError(res.status, body);

        // Ожидаемый гейт-отказ star (не-премиум): не шум, а подозрение —
        // сверочный запрос сам себя не триггерит.
        const starDeny = base === API_STAR_BASE && res.status === 403;
        if (starDeny && path !== '/me/subscription') requestPremiumRecheck();

        // Штатный по контракту статус (напр. 404 /related = соседей пока нет):
        // глушим тихо — без тоста, без recovery, без error-лога.
        if (silentStatuses?.includes(res.status)) throw err;

        if (!isLast && isHostFailover(res.status)) {
          lastError = err;
          continue;
        }

        logHttpError(label, res.status, url, body);

        // Rate-limit — копим, одиночный не дёргает recovery.
        if (isRateLimitError(res.status, body)) {
          noteRateLimit();
          console.error(`HTTP ERROR: url: ${path}, `, err);
          throw err;
        }

        // Протухший токен (401) либо юзер пропал из сайдбара — сильный
        // сигнал, silent renew сразу. Гейтовый 403 star recovery не дёргает.
        if (res.status === 401 || (useAuthStore.getState().user == null && !starDeny)) {
          noteAuthGap();
          console.error(`HTTP ERROR: url: ${path}, `, err);
          throw err;
        }

        if (!starDeny) handleApiError(err);
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
      markUnhealthy(base);
      if (isTimeoutError(error)) noteRequestTimeout();
      if (!isLast) {
        lastError = error;
        continue;
      }
      logHttpFailure(label, url, error, performance.now() - attemptStart);
      useAppStatusStore.getState().setBackendReachable(false);
      throw error;
    }
  }

  throw lastError ?? new Error('Request failed');
}

// ─── Aliases ────────────────────────────────────────────────

export const fetchWithAuthFallback = apiRequest;
