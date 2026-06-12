import {useQuery} from '@tanstack/react-query';
import {useAppStatusStore} from '../stores/app-status';
import {useAuthStore} from '../stores/auth';
import {api, getSessionId} from './api';
import {trackedInvoke as invoke} from './diagnostics';
import {getIsPremium, setIsPremium, setPremiumRecheckHandler} from './premium-cache';
import {queryClient} from './query-client';

export { getIsPremium } from './premium-cache';

interface SubscriptionResponse {
  premium: boolean;
}

const QUERY_KEY = ['me', 'subscription'] as const;

async function fetchSubscription(): Promise<SubscriptionResponse> {
  const token = getSessionId();
  try {
    const res = await api<SubscriptionResponse>('/me/subscription');
    // Session changed (logout / re-login) while we awaited — drop the result.
    if (getSessionId() !== token) return { premium: getIsPremium() };
    if (res.premium !== getIsPremium()) {
      setIsPremium(res.premium);
      // Premium подтверждён сетевым ответом — data-plane достижим, не ждём recheck-кулдауна.
      if (res.premium) useAppStatusStore.getState().setBackendReachable(true);
      void invoke('auth_set_premium', { premium: res.premium }).catch(() => {});
    }
    return res;
  } catch {
    // Network failure / both hosts down: keep cached value, don't reset to false
    return { premium: getIsPremium() };
  }
}

export function useSubscription(enabled: boolean) {
  return useQuery({
    queryKey: QUERY_KEY,
    queryFn: fetchSubscription,
    enabled,
    staleTime: 30_000,
    refetchInterval: 30_000,
    select: (d) => d.premium,
  });
}

// Сверка по подозрению (star 403 / вход в star-reserve): single-flight + cooldown.
// Гейт только hasSession (не isAuthenticated): при мёртвом main /me/cold не проходит,
// recheck — единственный путь восстановления premium у залогиненного юзера.
const RECHECK_COOLDOWN_MS = 30_000;
let recheckInFlight = false;
let lastRecheckAt = 0;
setPremiumRecheckHandler(() => {
  if (!useAuthStore.getState().hasSession) return;
  const now = Date.now();
  if (recheckInFlight || now - lastRecheckAt < RECHECK_COOLDOWN_MS) return;
  recheckInFlight = true;
  lastRecheckAt = now;
  void fetchSubscription()
    .then((d) => queryClient.setQueryData(QUERY_KEY, d))
    .finally(() => {
      recheckInFlight = false;
    });
});

// Eagerly fetch subscription on auth so getIsPremium() is ready before first track play
useAuthStore.subscribe((state, prev) => {
  if (state.isAuthenticated && !prev.isAuthenticated) {
    fetchSubscription().catch(() => {});
  }
});

// Fetch on startup if already authenticated (rehydrated session)
if (useAuthStore.getState().isAuthenticated) {
  fetchSubscription().catch(() => {});
}
