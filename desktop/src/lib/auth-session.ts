import {listen} from '@tauri-apps/api/event';
import {useAuthStore} from '../stores/auth';
import {useAuthRecoveryStore} from '../stores/auth-recovery';
import {setSessionId} from './api';
import {trackedInvoke as invoke} from './diagnostics';
import {queryClient} from './query-client';

interface ServerAuthState {
    token: string | null;
}

/**
 * Apply Rust-owned session state to the frontend mirrors. The api-client token
 * mirror and the auth store are written ONLY here — always with a value Rust
 * just confirmed (command return or the `auth:changed` broadcast). Idempotent,
 * so the initiating window and the tray can both run it.
 */
export function applyAuthFromServer(token: string | null): void {
    setSessionId(token);
    if (token) {
        useAuthStore.setState({hasSession: true});
    } else {
        useAuthStore.setState({hasSession: false, isAuthenticated: false, user: null});
        queryClient.clear();
        useAuthRecoveryStore.getState().reset();
    }
}

/** Seed the mirror from Rust once, then track every `auth:changed` broadcast. */
export async function initAuthBridge(): Promise<void> {
    try {
        const snap = await invoke<ServerAuthState>('auth_status');
        applyAuthFromServer(snap?.token ?? null);
    } catch {
        applyAuthFromServer(null);
    }
    await listen<ServerAuthState>('auth:changed', (e) => {
        applyAuthFromServer(e.payload?.token ?? null);
    });
}
