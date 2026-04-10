import { useQuery } from '@tanstack/react-query';
import { api } from './api';

interface SubscriptionResponse {
  premium: boolean;
}

const QUERY_KEY = ['me', 'subscription'] as const;

let cachedPremium = false;

export function getIsPremium(): boolean {
  return cachedPremium;
}

export function useSubscription(enabled: boolean) {
  return useQuery({
    queryKey: QUERY_KEY,
    queryFn: async () => {
      const res = await api<SubscriptionResponse>('/me/subscription');
      cachedPremium = res.premium;
      return res;
    },
    enabled,
    staleTime: 30_000,
    refetchInterval: 30_000,
    select: (d) => d.premium,
  });
}
