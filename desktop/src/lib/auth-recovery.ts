import { useAuthStore } from '../stores/auth';
import { useAuthRecoveryStore } from '../stores/auth-recovery';
import { queryClient } from './query-client';

/**
 * Оркестратор восстановления сессии.
 *
 * Триггеры (из api-client):
 *   - `noteRateLimit()` — rate-limit. НЕ реагируем на одиночный: накопитель,
 *     модалка только при устойчивом троттлинге (THRESHOLD за WINDOW).
 *   - `noteAuthGap()`   — протухший токен (401) ИЛИ юзер пропал из сайдбара.
 *     Это сильный сигнал → silent renew сразу.
 *   - `noteSuccess()`   — любой успешный ответ: чистит накопитель и, если
 *     всё само починилось, гасит pending-recovery / закрывает модалку.
 *
 * Стратегия: первая попытка — silent renew (без UI). Не помогло → модалка
 * (ручной retry / re-login). Single-flight по `inFlight` + `phase`.
 */

const RL_WINDOW_MS = 15_000;
const RL_THRESHOLD = 3;
const RECOVERED_COOLDOWN_MS = 5000;

let rlHits: number[] = [];
let inFlight: Promise<void> | null = null;
/** Поколение текущей silent-попытки — для отмены при само-восстановлении. */
let gen = 0;
let cancelledGen = -1;

async function runRenew(manual: boolean): Promise<void> {
  if (inFlight) return inFlight;

  const myGen = ++gen;
  const store = useAuthRecoveryStore.getState();
  if (manual) {
    store.setBusy(true);
  } else {
    store.setPhase('silent');
  }

  inFlight = (async () => {
    try {
      await useAuthStore.getState().renewSession();
      useAuthRecoveryStore.getState().markRecovered();
      queryClient.invalidateQueries();
    } catch {
      // Само-восстановилось параллельным успешным запросом — модалку не лепим.
      if (cancelledGen === myGen) return;
      const s = useAuthRecoveryStore.getState();
      s.setPhase('modal');
      s.setBusy(false);
    } finally {
      inFlight = null;
    }
  })();

  return inFlight;
}

function startRecovery(): void {
  const s = useAuthRecoveryStore.getState();
  if (s.phase !== 'idle') return;
  if (Date.now() - s.recoveredAt < RECOVERED_COOLDOWN_MS) return;
  void runRenew(false);
}

/** Rate-limit: накапливаем, эскалируем только при устойчивом троттлинге. */
export function noteRateLimit(): void {
  const now = Date.now();
  rlHits.push(now);
  rlHits = rlHits.filter((t) => now - t < RL_WINDOW_MS);
  if (rlHits.length >= RL_THRESHOLD) {
    rlHits = [];
    startRecovery();
  }
}

/** Протухший токен / юзер пропал из сайдбара — реагируем сразу. */
export function noteAuthGap(): void {
  startRecovery();
}

/**
 * Успешный ответ: чистим накопитель и, если всё ожило само, снимаем
 * pending-recovery или авто-закрываем модалку (но не во время ручного
 * renew / OAuth — там юзер сам рулит).
 */
export function noteSuccess(): void {
  if (rlHits.length) rlHits = [];
  const s = useAuthRecoveryStore.getState();
  if (s.phase === 'idle' || s.busy || s.oauthActive) return;
  cancelledGen = gen;
  s.markRecovered();
}

/** Ручной повтор renew из модалки. */
export function retryRenew(): Promise<void> {
  return runRenew(true);
}

/** Успешный полный re-login (OAuth). */
export function completeReauth(sessionId: string): void {
  const auth = useAuthStore.getState();
  auth.setSession(sessionId);
  auth.fetchUser().catch(() => {});
  useAuthRecoveryStore.getState().markRecovered();
  queryClient.invalidateQueries();
}
