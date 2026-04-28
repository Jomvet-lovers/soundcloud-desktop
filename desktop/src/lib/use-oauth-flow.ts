import { fetch } from '@tauri-apps/plugin-http';
import { openUrl } from '@tauri-apps/plugin-opener';
import { useCallback, useEffect, useRef, useState } from 'react';
import { fetchWithAuthFallback } from './api-client';
import { API_BASE, BYPASS_API_BASE } from './constants';

interface LoginResponse {
  url: string;
  loginRequestId: string;
}

interface LoginStatusResponse {
  status: 'pending' | 'completed' | 'failed' | 'expired';
  sessionId?: string;
  error?: string;
}

export type OAuthFlowError = { kind: 'failed' | 'expired'; message: string };

export function useOAuthFlow(
  onSuccess: (sessionId: string) => void,
  onFailure?: (err: OAuthFlowError) => void,
) {
  const [authUrl, setAuthUrl] = useState<string | null>(null);
  const [isPolling, setIsPolling] = useState(false);
  const pollRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onSuccessRef = useRef(onSuccess);
  const onFailureRef = useRef(onFailure);
  onSuccessRef.current = onSuccess;
  onFailureRef.current = onFailure;

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

    // x-session-id (если есть) автоматически уйдёт через apiRequest — тогда бэк
    // привяжет результат к существующей сессии и sessionId не сменится.
    const { url, loginRequestId } = await fetchWithAuthFallback<LoginResponse>('/auth/login');
    setAuthUrl(url);
    await openUrl(url);

    const tryPoll = async (base: string): Promise<LoginStatusResponse | null> => {
      const controller = new AbortController();
      const timer = setTimeout(() => controller.abort(), 10_000);
      try {
        const res = await fetch(
          `${base}/auth/login/status?id=${encodeURIComponent(loginRequestId)}`,
          { signal: controller.signal },
        );
        if (!res.ok) return null;
        return (await res.json()) as LoginStatusResponse;
      } finally {
        clearTimeout(timer);
      }
    };

    const pollOnce = async () => {
      let data: LoginStatusResponse | null = null;
      try {
        data = await tryPoll(API_BASE);
      } catch {
        try {
          data = await tryPoll(BYPASS_API_BASE);
        } catch {}
      }

      if (!data) {
        pollRef.current = setTimeout(pollOnce, 2000);
        return;
      }

      if (data.status === 'completed' && data.sessionId) {
        cancel();
        onSuccessRef.current(data.sessionId);
        return;
      }
      if (data.status === 'failed' || data.status === 'expired') {
        cancel();
        onFailureRef.current?.({ kind: data.status, message: data.error ?? 'Login failed' });
        return;
      }
      pollRef.current = setTimeout(pollOnce, 2000);
    };

    pollRef.current = setTimeout(pollOnce, 2000);
  }, [cancel]);

  return { startLogin, authUrl, isPolling, cancel };
}
