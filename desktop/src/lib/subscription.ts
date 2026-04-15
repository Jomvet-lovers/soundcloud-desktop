import { useQuery } from '@tanstack/react-query';
import { useAuthStore } from '../stores/auth';
import { useSettingsStore } from '../stores/settings';
import { api } from './api';
import { getIsPremium, setIsPremium } from './premium-cache';

export { getIsPremium } from './premium-cache';

interface SubscriptionResponse {
  premium: boolean;
}

const QUERY_KEY = ['me', 'subscription'] as const;

async function fetchSubscription(): Promise<SubscriptionResponse> {
  try {
    const res = await api<SubscriptionResponse>('/me/subscription');
    setIsPremium(res.premium);
    // Auto-disable bypass if no longer premium
    if (!res.premium && useSettingsStore.getState().bypassWhitelist) {
      useSettingsStore.getState().setBypassWhitelist(false);
    }
    return res;
  } catch {
    // Network failure: keep cached value, don't reset to false
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
