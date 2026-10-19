import { listen } from '@tauri-apps/api/event';
import { create } from 'zustand';
import { fetchWithAuthFallback, getSessionId } from '../lib/api';
import { applyAuthFromServer, readAuthStatus } from '../lib/auth-session';
import { trackedInvoke as invoke } from '../lib/diagnostics';
import { resetDislikedUrns } from '../lib/dislikes';
import { preferredControlBase } from '../lib/host-status';
import { resetLikedUrns } from '../lib/likes';
import { setOfflineAccount } from '../lib/offline-index';
import { queryClient } from '../lib/query-client';

export interface User {
  id: number;
  urn: string;
  username: string;
  avatar_url: string;
  permalink_url: string;
  followers_count: number;
  followings_count: number;
  track_count: number;
  playlist_count: number;
  public_favorites_count?: number;
  likes_count?: number;
}

export interface Account {
  id: number;
  token: string;
  username: string;
  avatar_url: string;
  premium: boolean;
}

interface AuthState {
  /** Validated session — `/me` resolved. Gates premium/auth-only UI. */
  isAuthenticated: boolean;
  /** Token present (Rust-owned). Lets the shell render before `/me` lands. */
  hasSession: boolean;
  user: User | null;
  /** All signed-in accounts, mirrored from Rust. */
  accounts: Account[];
  /** SoundCloud user id of the active account, or null when signed out. */
  activeId: number | null;
  /** Active account token, mirrored from Rust. Drives `/me` refetches. */
  token: string | null;
  setSession: (token: string) => Promise<void>;
  fetchUser: () => Promise<void>;
  renewSession: () => Promise<void>;
  switchAccount: (id: number) => Promise<void>;
  removeAccount: (id: number) => Promise<void>;
  addAccount: () => Promise<void>;
  logout: () => Promise<void>;
}

/** Per-account mirrors: never let the previous account's caches survive. */
function resetAccountCaches() {
  resetLikedUrns();
  resetDislikedUrns();
  queryClient.clear();
}

export const useAuthStore = create<AuthState>()((set, get) => ({
  isAuthenticated: false,
  hasSession: false,
  user: null,
  accounts: [],
  activeId: null,
  token: null,

  setSession: async (token: string) => {
    await invoke('auth_set_session', { token });
    // Re-read the Rust view instead of assuming the token argument is the
    // active one: the command upserts the account before emitting.
    const snap = await readAuthStatus();
    if (snap) applyAuthFromServer(snap);
  },

  fetchUser: async () => {
    const token = getSessionId();
    if (!token) return;
    const user = await fetchWithAuthFallback<User>('/me/cold');
    // Session changed (logout / re-login) while we awaited — drop the result.
    if (getSessionId() !== token) return;
    set({ user, isAuthenticated: true });
  },

  renewSession: async () => {
    await fetchWithAuthFallback('/auth/refresh', { method: 'POST' });
    await get().fetchUser();
  },

  switchAccount: async (id: number) => {
    await invoke('auth_switch_account', { id });
    // The store itself is updated by `auth:changed`, but per-account caches
    // must be dropped before `/me` and the dislikes list refetch.
    resetAccountCaches();
    await setOfflineAccount(id);
    await get().fetchUser();
  },

  removeAccount: async (id: number) => {
    await invoke('auth_remove_account', { id });
  },

  addAccount: async () => {
    await new Promise<void>((resolve, reject) => {
      let unlisten: (() => void) | null = null;
      let settled = false;
      const finish = (error?: Error) => {
        if (settled) return;
        settled = true;
        unlisten?.();
        unlisten = null;
        if (error) reject(error);
        else resolve();
      };

      void listen<{ status: 'ok' | 'error' | 'cancel'; message?: string }>('sc-login', (event) => {
        if (event.payload.status === 'error') {
          finish(new Error(event.payload.message ?? 'Sign-in failed'));
        } else {
          finish();
        }
      })
        .then((stop) => {
          if (settled) {
            stop();
            return;
          }
          unlisten = stop;
          invoke('open_login_window').catch((error) => {
            finish(error instanceof Error ? error : new Error(String(error)));
          });
        })
        .catch((error) => finish(error instanceof Error ? error : new Error(String(error))));
    });
  },

  logout: async () => {
    // Rust removes the active account and emits the remaining view.
    await invoke('auth_logout', { apiBase: preferredControlBase() });
  },
}));
