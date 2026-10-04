import {type ReactNode, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {useNavigate} from 'react-router-dom';
import {AuthBackdrop} from '../components/auth/AuthBackdrop';
import {BrandMark} from '../components/auth/BrandMark';
import {OfflineEntryCard} from '../components/auth/OfflineEntryCard';
import {QrLinkSheet} from '../components/auth/QrLinkSheet';
import {
    AlertCircle,
    Check,
    ChevronRight,
    ClipboardCopy,
    RefreshCw,
    Smartphone,
} from '../lib/icons';
import { trackedInvoke as invoke } from '../lib/diagnostics';
import {DIRECT_MODE} from '../lib/constants';
import {queryClient} from '../lib/query-client';
import {useOAuthFlow} from '../lib/use-oauth-flow';
import {useAppStatusStore} from '../stores/app-status';
import {useAuthStore} from '../stores/auth';

export function Login() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const setSession = useAuthStore((s) => s.setSession);
  const fetchUser = useAuthStore((s) => s.fetchUser);
  const setOfflineBypass = useAppStatusStore((s) => s.setOfflineBypass);
  const [copied, setCopied] = useState(false);
  const [qrOpen, setQrOpen] = useState(false);
  const [tokenInput, setTokenInput] = useState('');
  const [tokenBusy, setTokenBusy] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);

  const handleDirectLogin = async () => {
    const value = tokenInput.trim();
    if (!value || tokenBusy) return;
    setTokenBusy(true);
    setTokenError(null);
    try {
      await invoke<string>('direct_login', { token: value });
      // Apply the session to the frontend mirror synchronously (the
      // auth:changed event may still be in flight when fetchUser runs).
      await setSession(value);
      setOfflineBypass(false);
      await fetchUser();
      queryClient.invalidateQueries();
    } catch (e) {
      setTokenError(e instanceof Error ? e.message : String(e));
    } finally {
      setTokenBusy(false);
    }
  };

  const handleEnterOffline = () => {
    setOfflineBypass(true);
    navigate('/offline', { replace: true });
  };

  const onLoginSuccess = async (sessionId: string) => {
      await setSession(sessionId);
    await fetchUser();
    queryClient.invalidateQueries();
  };

  const { startLogin, authUrl, isPolling, step, error } = useOAuthFlow(onLoginSuccess);

  const handleLogin = async () => {
    try {
      await startLogin();
    } catch (e) {
      console.error('Login failed:', e);
    }
  };

  const errorTitle = !error
    ? ''
    : error.kind === 'unreachable'
      ? t('auth.errorServerTitle')
      : error.kind === 'expired'
        ? t('auth.errorExpiredTitle')
        : t('auth.errorFailedTitle');
  const errorDesc =
    error?.kind === 'unreachable' ? t('auth.errorServerDesc') : (error?.message ?? '');

  const stepLabel =
    step === 'token'
      ? t('auth.stepToken')
      : step === 'profile'
        ? t('auth.stepProfile')
        : step === 'session'
          ? t('auth.stepSession')
          : t('auth.stepWaiting');

  return (
    <div className="h-screen flex items-center justify-center relative overflow-hidden">
        <AuthBackdrop/>

        <div className="relative z-10 w-full max-w-[400px] mx-4" style={{isolation: 'isolate'}}>
            <div
                className="relative overflow-hidden rounded-[2.25rem] px-8 pt-9 pb-7"
                style={{
                    border: '0.5px solid rgba(255,255,255,0.1)',
                    background: 'rgba(18, 18, 22, 0.97)',
                    boxShadow: '0 24px 60px rgba(0,0,0,0.5)',
                }}
            >
          <span
              aria-hidden
              className="absolute inset-x-8 top-0 h-px"
              style={{
                  background: 'linear-gradient(90deg, transparent, rgba(255,255,255,0.3), transparent)',
              }}
          />

                <BrandMark subtitle={isPolling ? t('auth.signingIn') : t('auth.tagline')}/>

                <div className="mt-8">
                    {error ? (
                        <div className="flex flex-col items-stretch gap-4">
                            <div
                                className="flex flex-col items-center gap-3 rounded-2xl border border-red-500/20 bg-red-500/[0.06] px-5 py-5 text-center">
                                <div
                                    className="flex size-11 items-center justify-center rounded-full border border-red-500/25 bg-red-500/10">
                                    <AlertCircle size={20} className="text-red-400" strokeWidth={1.8}/>
                                </div>
                                <div>
                                    <p className="text-[14px] font-semibold text-white/90">{errorTitle}</p>
                                    <p className="mt-1 text-[12px] leading-snug text-white/45 break-words">
                                        {errorDesc}
                                    </p>
                                </div>
                            </div>
                            <PrimaryButton onClick={handleLogin}>
                                <RefreshCw size={15} strokeWidth={2}/>
                                {t('auth.retry')}
                            </PrimaryButton>
                            <OfflineEntryCard onClick={handleEnterOffline}/>
                        </div>
                    ) : isPolling ? (
                        <div className="flex flex-col items-center gap-4 py-2">
                            <div
                                className="w-10 h-10 rounded-full border-2 border-white/[0.08] border-t-accent animate-spin"/>
                            <p className="text-[12px] text-white/45">{stepLabel}</p>
                            {authUrl && (
                                <button
                                    type="button"
                                    onClick={() => {
                                        navigator.clipboard.writeText(authUrl);
                                        setCopied(true);
                                        setTimeout(() => setCopied(false), 2000);
                                    }}
                                    className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl bg-white/[0.04] hover:bg-white/[0.08] border border-white/[0.06] text-[11px] text-white/40 hover:text-white/60 transition-all cursor-pointer"
                                >
                                    {copied ? (
                                        <>
                                            <Check size={12}/>
                                            {t('auth.copied')}
                                        </>
                                    ) : (
                                        <>
                                            <ClipboardCopy size={12}/>
                                            {t('auth.copyLink')}
                                        </>
                                    )}
                                </button>
                            )}
                        </div>
                    ) : (
                        <div className="flex flex-col items-stretch gap-3">
                            {!DIRECT_MODE && (
                                <>
                                    <PrimaryButton onClick={handleLogin}>
                                        {t('auth.signIn')}
                                        <ChevronRight size={16} strokeWidth={2.4}/>
                                    </PrimaryButton>
                                    <button
                                        type="button"
                                        onClick={() => setQrOpen(true)}
                                        className="w-full flex items-center justify-center gap-2 py-2.5 rounded-xl text-[12.5px] font-medium text-white/45 hover:text-white/80 hover:bg-white/[0.04] transition-all cursor-pointer"
                                    >
                                        <Smartphone size={14}/>
                                        {t('qrLink.scanQr')}
                                    </button>

                                    <div
                                        className="my-1 flex items-center gap-3 text-[10px] uppercase tracking-[0.22em] text-white/25">
                                        <div className="h-px flex-1 bg-gradient-to-r from-transparent to-white/10"/>
                                        {t('auth.orSeparator')}
                                        <div className="h-px flex-1 bg-gradient-to-l from-transparent to-white/10"/>
                                    </div>
                                </>
                            )}

                            <OfflineEntryCard onClick={handleEnterOffline}/>

                            <div className="mt-1 flex flex-col gap-2">
                                <p className="text-[10.5px] leading-snug text-white/30">
                                    {t('auth.directHint')}
                                </p>
                                <input
                                    type="password"
                                    value={tokenInput}
                                    onChange={(e) => setTokenInput(e.target.value)}
                                    onKeyDown={(e) => {
                                        if (e.key === 'Enter') void handleDirectLogin();
                                    }}
                                    placeholder={t('auth.directTokenPlaceholder')}
                                    className="w-full rounded-xl border border-white/[0.08] bg-white/[0.05] px-3 py-2.5 text-[12px] text-white/80 outline-none transition-colors placeholder:text-white/25 focus:border-white/20"
                                />
                                {tokenError && (
                                    <p className="break-words text-[11px] text-red-400/80">{tokenError}</p>
                                )}
                                <button
                                    type="button"
                                    disabled={tokenBusy || !tokenInput.trim()}
                                    onClick={() => void handleDirectLogin()}
                                    className="w-full rounded-xl border border-white/[0.08] bg-white/[0.04] py-2.5 text-[12px] font-medium text-white/60 transition-all hover:bg-white/[0.08] hover:text-white/85 disabled:cursor-not-allowed disabled:opacity-40"
                                >
                                    {tokenBusy ? t('auth.directConnecting') : t('auth.directConnect')}
                                </button>
                            </div>
                        </div>
                    )}
                </div>
            </div>
      </div>

      <QrLinkSheet open={qrOpen} onOpenChange={setQrOpen} mode="pull" onSuccess={onLoginSuccess} />
    </div>
  );
}

/** Primary accent CTA (flat). */
function PrimaryButton({
                           onClick,
                           children,
                       }: {
    onClick: () => void;
    children: ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="group relative w-full h-12 overflow-hidden rounded-2xl text-sm font-bold cursor-pointer transition-transform duration-200 ease-[var(--ease-apple)] hover:scale-[1.02] active:scale-[0.97]"
      style={{
          color: 'var(--color-accent-contrast)',
          background: 'var(--color-accent)',
      }}
    >
        <span className="relative flex items-center justify-center gap-2">{children}</span>
    </button>
  );
}
