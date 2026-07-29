import { toast } from 'sonner';
import i18n from '../i18n';
import { useAppStatusStore } from '../stores/app-status';
import { useAuthStore } from '../stores/auth';
import { noteAuthGap, noteRateLimit, noteSuccess } from './auth-recovery';
import { API_BASE, API_STAR_BASE } from './constants';
import { logHttpError, logHttpFailure, trackAsync } from './diagnostics';
import { edgeFetch } from './edge';
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

function fetchWithTimeout(url: string, options: RequestInit, timeoutMs: number): Promise<Response> {
  return edgeFetch(url, options, timeoutMs);
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

/**
 * Бюджеты — по замерам на проде (30.07.2026), а не на глаз. Причина, по которой
 * авторизованные ручки медленные, одна: `get_valid_session` на бэке при протухшем
 * токене держит per-session мьютекс, пока ходит в SoundCloud за новым. Клиент в
 * этот момент шлёт десятки запросов, и все они стоят в этой очереди. Так что
 * «медленно» здесь — штатное поведение, а не признак смерти хоста.
 *
 * `/auth/*`  — бэк round-trip'ит в SoundCloud: login замерен 4.2–11.7 с.
 * `/me/subscription` — сама ручка это один SELECT, но под конвоем refresh-лока
 *   в логах видны легитимные 17.9 с.
 * data-plane — `/recommendations` доходил до 49.8 с.
 *
 * Хост, уже признанный мёртвым, всё это не ждёт: `DOWN_HOST_TIMEOUT_MS`.
 */
const AUTH_TIMEOUT_MS = 30_000;
const SUBSCRIPTION_TIMEOUT_MS = 30_000;
const DATA_PLANE_TIMEOUT_MS = 90_000;
const DOWN_HOST_TIMEOUT_MS = 10_000;

/**
 * Базы запроса, primary первой.
 * /me/subscription — открыт на star (bootstrap-сигнал премиума): перебор обоих, primary по вердикту main.
 * /auth/login* — прибит к main: OAuth redirect_uri зарегистрирован в SC только на основной хост.
 * остальной /auth/* (refresh, link/*, logout) — ровно ОДИН хост по вердикту: refresh нельзя
 *   фейловерить перебором (per-process refresh_locks на бэке → двойная ротация refresh_token),
 *   link-токены single-use.
 * data-plane — как раньше: премиум → star, GET/HEAD с фолбэком. Мутации тоже
 * знают main как страховку, но переключаются туда только на явной
 * инфраструктурной HTML-ошибке (запрос не дошёл до приложения).
 */
function apiBasesFor(path: string): string[] {
  if (path === '/me/subscription') {
    return getHostVerdict(API_BASE) === 'down'
      ? [API_STAR_BASE, API_BASE]
      : [API_BASE, API_STAR_BASE];
  }
  if (path.startsWith('/auth/login')) return [API_BASE];
  if (path.startsWith('/auth/')) return [preferredControlBase()];
  if (getIsPremium() && sessionId && isHealthy(API_STAR_BASE)) {
    return [API_STAR_BASE, API_BASE];
  }
  return [API_BASE];
}

/** Бюджет запроса по его пути. */
function planeTimeout(path: string): number {
  if (path.startsWith('/auth/')) return AUTH_TIMEOUT_MS;
  if (path === '/me/subscription') return SUBSCRIPTION_TIMEOUT_MS;
  return DATA_PLANE_TIMEOUT_MS;
}

/** Host-фейл → фейловер. 4xx-контракты (400/404/…) — валидный ответ, не фейлим. */
function isHostFailover(status: number): boolean {
  return status >= 500 || status === 401 || status === 403;
}

/**
 * Load-balancer/proxy failures are safe to replay even for a mutation: the
 * HTML error page means the request never reached the API application. JSON
 * 5xx responses remain application errors and are not replayed.
 */
function isInfrastructureFailure(status: number, body: string): boolean {
  if (status < 500) return false;
  const b = body.toLowerCase();
  return b.includes('<html') || b.includes('no server is available');
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
  const bases = apiBasesFor(path);
  const effectiveTimeout = timeoutMs ?? planeTimeout(path);
  let lastError: unknown = null;

  for (let i = 0; i < bases.length; i++) {
    const base = bases[i];
    const isLast = i === bases.length - 1;
    const url = `${base}${path}`;
    const attemptStart = performance.now();

    // Хост с вердиктом down не держит попытку дольше 10 c.
    const attemptTimeout =
      getHostVerdict(base) === 'down'
        ? Math.min(effectiveTimeout, DOWN_HOST_TIMEOUT_MS)
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

        const idempotent = method === 'GET' || method === 'HEAD';
        const canFailover = idempotent || isInfrastructureFailure(res.status, body);
        if (!isLast && isHostFailover(res.status) && canFailover) {
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
        //
        // 5xx сюда НЕ попадает: «сервер сломался» не значит «сессия умерла».
        // На холодном старте `user` ещё null, и бэкендовый 502 (в т.ч. штатный
        // «Renewing your session, try again shortly») уводил в recovery, а через
        // две тихих попытки — в модалку «сессия истекла».
        const looksLikeAuthGap =
          res.status === 401 ||
          (res.status < 500 && useAuthStore.getState().user == null && !starDeny);
        if (looksLikeAuthGap) {
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
