import { listen } from '@tauri-apps/api/event';
import type { Account } from '../stores/auth';
import { useAuthStore } from '../stores/auth';
import { useAuthRecoveryStore } from '../stores/auth-recovery';
import { setSessionId } from './api';
import { trackedInvoke as invoke } from './diagnostics';
import { resetDislikedUrns } from './dislikes';
import { resetLikedUrns } from './likes';
import { setOfflineAccount } from './offline-index';
import { setIsPremium } from './premium-cache';
import { queryClient } from './query-client';

export interface AuthView {
  accounts: Account[];
  active: number | null;
  token: string | null;
  premium?: boolean;
}

/** Caches keyed by the active account must not leak into the next one. */
function resetAccountScopedState(): void {
  resetLikedUrns();
  resetDislikedUrns();
  queryClient.clear();
}

/**
 * Apply Rust-owned session state to the frontend mirrors. The api-client token
 * mirror and the auth store are written ONLY here — always with a view Rust
 * just confirmed (command return or the `auth:changed` broadcast). Premium
 * mirror: `undefined` leaves it untouched (caller without the flag). A view
 * without a token means no active account (last sign-out), so it lands on
 * <Login/> even though Rust still sends the (empty) account list. Idempotent,
 * so the initiating window and the tray can both run it.
 */
export function applyAuthFromServer(view: AuthView | null): void {
  const previousActiveId = useAuthStore.getState().activeId;
  const accounts = view?.accounts ?? [];
  const activeId = view?.active ?? null;
  const token = view?.token ?? null;

  if (!token) {
    setSessionId(null);
    setIsPremium(false);
    resetAccountScopedState();
    void setOfflineAccount(null);
    useAuthStore.setState({
      hasSession: false,
      isAuthenticated: false,
      user: null,
      token: null,
      accounts,
      activeId,
    });
    useAuthRecoveryStore.getState().reset();
    return;
  }

  setSessionId(token);
  if (view?.premium !== undefined) setIsPremium(view.premium);
  if (activeId !== previousActiveId) {
    resetAccountScopedState();
    useAuthRecoveryStore.getState().reset();
    // Do not show the previous account's profile while `/me` for the new
    // account is in flight; `hasSession` keeps the shell rendered.
    useAuthStore.setState({ user: null, isAuthenticated: false });
    void setOfflineAccount(activeId);
  }
  useAuthStore.setState({ hasSession: true, token, accounts, activeId });
}

const SNAPSHOT_ATTEMPTS = 3;
const SNAPSHOT_RETRY_MS = 300;

const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

/**
 * Снимок сессии из Rust. Провал invoke — это НЕ «юзер разлогинен», а «Rust не
 * ответил», и путать их нельзя: в dev webview перезагружается отдельно от
 * бэкенда, висящий callback теряется (`[TAURI] Couldn't find callback id`), и
 * ошибка прилетает на ровном месте. Раньше её принимали за пустую сессию —
 * зеркало чистилось, и первые запросы уходили без `x-session-id`, ловя
 * бессмысленный 401 «Missing or malformed». Поэтому: ретраим, а не отвечаем.
 */
export async function readAuthStatus(): Promise<AuthView | null> {
  for (let attempt = 1; attempt <= SNAPSHOT_ATTEMPTS; attempt++) {
    try {
      return await invoke<AuthView>('auth_status');
    } catch {
      if (attempt < SNAPSHOT_ATTEMPTS) await sleep(SNAPSHOT_RETRY_MS);
    }
  }
  return null;
}

/** Seed the mirror from Rust once, then track every `auth:changed` broadcast. */
export async function initAuthBridge(): Promise<void> {
  // Подписка ПЕРЕД снимком: логин/логаут, случившийся пока читаем снимок,
  // иначе потерялся бы целиком.
  await listen<AuthView>('auth:changed', (e) => {
    applyAuthFromServer(e.payload ?? null);
  });
  const snap = await readAuthStatus();
  // Rust так и не ответил — зеркало не трогаем. Пусть его заполнит
  // `auth:changed`, а запросы подождут (`api-client`: гейт с потолком).
  if (snap) applyAuthFromServer(snap);
}
