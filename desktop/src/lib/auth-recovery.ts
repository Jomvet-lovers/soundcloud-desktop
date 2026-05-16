import { useAuthStore } from '../stores/auth';
import { useAuthRecoveryStore } from '../stores/auth-recovery';
import { queryClient } from './query-client';

/**
 * Оркестратор восстановления сессии.
 *
 * Единая точка реакции на auth-recoverable ошибки (rate-limit, протухший
 * access-токен, пустой сайдбар). Стратегия:
 *
 *   1. Первая попытка — silent renew (POST /auth/refresh + /me). Без модалки.
 *   2. Не помогло → модалка: ручной retry renew ИЛИ полный re-login (OAuth).
 *
 * Single-flight: параллельные failing-запросы (а их при пустом сайдбаре
 * пачка) не плодят повторные renew — все, кроме первого, отскакивают по
 * `phase !== 'idle'` и по `inFlight`.
 */

const RECOVERED_COOLDOWN_MS = 5000;

let inFlight: Promise<void> | null = null;

async function runRenew(manual: boolean): Promise<void> {
  if (inFlight) return inFlight;

  const store = useAuthRecoveryStore.getState();
  if (manual) {
    store.setBusy(true);
  } else {
    store.setPhase('silent');
  }

  inFlight = (async () => {
    try {
      await useAuthStore.getState().renewSession();
      // renew прошёл и /me снова ответил — гасим всё, рефетчим данные.
      useAuthRecoveryStore.getState().markRecovered();
      queryClient.invalidateQueries();
    } catch {
      // renew не помог → показываем модалку (ручной retry / re-login).
      const s = useAuthRecoveryStore.getState();
      s.setPhase('modal');
      s.setBusy(false);
    } finally {
      inFlight = null;
    }
  })();

  return inFlight;
}

/**
 * Вызывается из api-client при auth-recoverable ошибке. Идемпотентно:
 * запускает silent-renew только из состояния `idle` и вне cooldown.
 */
export function recoverSession(): void {
  const s = useAuthRecoveryStore.getState();
  if (s.phase !== 'idle') return;
  if (Date.now() - s.recoveredAt < RECOVERED_COOLDOWN_MS) return;
  void runRenew(false);
}

/** Ручной повтор renew из модалки. */
export function retryRenew(): Promise<void> {
  return runRenew(true);
}

/**
 * Успешный полный re-login (OAuth). Прокидывает сессию и гасит модалку.
 */
export function completeReauth(sessionId: string): void {
  const auth = useAuthStore.getState();
  auth.setSession(sessionId);
  auth.fetchUser().catch(() => {});
  useAuthRecoveryStore.getState().markRecovered();
  queryClient.invalidateQueries();
}
