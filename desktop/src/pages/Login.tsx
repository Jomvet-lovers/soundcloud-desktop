import { listen } from '@tauri-apps/api/event';
import { type ReactNode, useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { AuthBackdrop } from '../components/auth/AuthBackdrop';
import { BrandMark } from '../components/auth/BrandMark';
import { OfflineEntryCard } from '../components/auth/OfflineEntryCard';
import { trackedInvoke as invoke } from '../lib/diagnostics';
import { AlertCircle } from '../lib/icons';
import { queryClient } from '../lib/query-client';
import { useAppStatusStore } from '../stores/app-status';
import { useAuthStore } from '../stores/auth';

export function Login() {
  const navigate = useNavigate();
  const setSession = useAuthStore((s) => s.setSession);
  const fetchUser = useAuthStore((s) => s.fetchUser);
  const setOfflineBypass = useAppStatusStore((s) => s.setOfflineBypass);
  const [loginError, setLoginError] = useState<string | null>(null);
  const [windowBusy, setWindowBusy] = useState(false);
  const windowBusyRef = useRef(false);
  const unlistenRef = useRef<(() => void) | null>(null);

  const handleEnterOffline = () => {
    setOfflineBypass(true);
    navigate('/offline', { replace: true });
  };

  useEffect(() => () => unlistenRef.current?.(), []);

  /** Sign in through a real SoundCloud web session in an in-app window. */
  const handleBrowserLogin = async () => {
    if (windowBusyRef.current) return;
    windowBusyRef.current = true;
    setWindowBusy(true);
    setLoginError(null);
    unlistenRef.current?.();
    const unlisten = await listen<{
      status: 'ok' | 'error' | 'cancel';
      token?: string;
      username?: string;
      message?: string;
    }>('sc-login', async (e) => {
      unlisten();
      unlistenRef.current = null;
      windowBusyRef.current = false;
      setWindowBusy(false);
      if (e.payload.status === 'ok' && e.payload.token) {
        try {
          await setSession(e.payload.token);
          setOfflineBypass(false);
          await fetchUser();
          queryClient.invalidateQueries();
        } catch (err) {
          setLoginError(err instanceof Error ? err.message : String(err));
        }
        return;
      }
      if (e.payload.status === 'error') {
        setLoginError(e.payload.message ?? 'Sign-in failed');
      }
    });
    unlistenRef.current = unlisten;
    try {
      await invoke('open_login_window');
    } catch (err) {
      unlisten();
      unlistenRef.current = null;
      windowBusyRef.current = false;
      setWindowBusy(false);
      setLoginError(err instanceof Error ? err.message : String(err));
    }
  };

  return (
    <div
      className="h-screen flex items-center justify-center relative overflow-hidden"
      data-tauri-drag-region
    >
      <AuthBackdrop />

      <div className="relative z-10 w-full max-w-[400px] mx-4" style={{ isolation: 'isolate' }}>
        <div
          className="relative overflow-hidden rounded-[2.25rem] px-8 pt-9 pb-7"
          style={{
            border: '0.5px solid rgba(255,255,255,0.1)',
            background: 'rgba(18, 18, 22, 0.97)',
            boxShadow: '0 24px 60px rgba(0,0,0,0.5)',
          }}
        >
          <BrandMark subtitle={'Your music, your way'} />

          <div className="mt-8 flex flex-col items-stretch gap-3">
            {loginError && (
              <div className="flex flex-col items-center gap-3 rounded-2xl border border-red-500/20 bg-red-500/[0.06] px-5 py-4 text-center">
                <div className="flex size-10 items-center justify-center rounded-full border border-red-500/25 bg-red-500/10">
                  <AlertCircle size={18} className="text-red-400" strokeWidth={1.8} />
                </div>
                <p className="break-words text-[12px] leading-snug text-white/60">{loginError}</p>
              </div>
            )}

            <PrimaryButton disabled={windowBusy} onClick={handleBrowserLogin}>
              {windowBusy ? 'Waiting for sign-in…' : 'Sign in with SoundCloud'}
            </PrimaryButton>

            <div className="my-1 flex items-center gap-3 text-[10px] text-white/25">
              <div className="h-px flex-1 bg-white/10" />
              {'or'}
              <div className="h-px flex-1 bg-white/10" />
            </div>

            <OfflineEntryCard onClick={handleEnterOffline} />
          </div>
        </div>
      </div>
    </div>
  );
}

/** Primary accent CTA (flat). */
function PrimaryButton({
  onClick,
  disabled,
  children,
}: {
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="w-full h-12 rounded-2xl text-sm font-bold cursor-pointer transition-opacity hover:opacity-90 active:opacity-80 disabled:cursor-not-allowed disabled:opacity-40"
      style={{
        color: 'var(--color-accent-contrast)',
        background: 'var(--color-accent)',
      }}
    >
      <span className="flex items-center justify-center gap-2">{children}</span>
    </button>
  );
}
