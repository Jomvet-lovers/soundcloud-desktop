import { fetch } from '@tauri-apps/plugin-http';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useCallback, useEffect, useRef, useState } from 'react';
import { fetchWithAuthFallback } from './api-client';
import { API_BASE, BYPASS_API_BASE } from './constants';

interface LoginResponse {
  url: string;
  sessionId: string;
}

interface SessionResponse {
  authenticated: boolean;
}

export function useOAuthFlow(onSuccess: (sessionId: string) => void) {
  const [authUrl, setAuthUrl] = useState<string | null>(null);
  const [isPolling, setIsPolling] = useState(false);
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onSuccessRef = useRef(onSuccess);
  onSuccessRef.current = onSuccess;

  const cancel = useCallback(() => {
    if (pollRef.current) {
      clearTimeout(pollRef.current);
      pollRef.current = null;
    }
    setIsPolling(false);
    setAuthUrl(null);
  }, []);

  useEffect(() => cancel, [cancel]);

  const startLogin = useCallback(async () => {
    cancel();
    setIsPolling(true);

    const { url, sessionId } = await fetchWithAuthFallback<LoginResponse>('/auth/login');
    setAuthUrl(url);
    await openUrl(url);

    const tryPoll = async (base: string) => {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 10_000);
      try {
        const res = await fetch(`${base}/auth/session`, {
          headers: { 'x-session-id': sessionId },
          signal: controller.signal,
        });
        return (await res.json()) as SessionResponse;
      } finally {
        clearTimeout(timer);
      }
    };

    const pollSession = async () => {
      try {
        let data: SessionResponse | null = null;
        try {
          data = await tryPoll(API_BASE);
        } catch {
          try {
            data = await tryPoll(BYPASS_API_BASE);
          } catch {}
        }
        if (data?.authenticated) {
          cancel();
          onSuccessRef.current(sessionId);
          return;
        }
      } catch {}
      pollRef.current = setTimeout(pollSession, 2000);
    };

    pollRef.current = setTimeout(pollSession, 2000);
  }, [cancel]);

  return { startLogin, authUrl, isPolling, cancel };
}
