import { useQuery } from '@tanstack/react-query';
import { api } from './api';

export type AuthTokenState = 'ok' | 'stale' | 'expired';

export interface AuthStatus {
  authenticated: boolean;
  sessionId?: string;
  username?: string;
  soundcloudUserId?: string;
  oauthAppId?: string;
  expiresAt?: string;
  expiresInSec?: number;
  tokenState: AuthTokenState;
  pendingSyncCount: number;
  failedSyncCount: number;
}

const POLL_MS = 30_000;

/**
 * Auth state of the current session. Includes token freshness and background
 * sync-queue counters. Polled in the background — we never hit sync_queue from the frontend.
 */
export function useAuthStatus(opts?: { enabled?: boolean }) {
  return useQuery<AuthStatus>({
    queryKey: ['auth', 'status'],
    queryFn: () => api<AuthStatus>('/auth/status'),
    staleTime: POLL_MS / 2,
    refetchInterval: POLL_MS,
    enabled: opts?.enabled ?? true,
  });
}
