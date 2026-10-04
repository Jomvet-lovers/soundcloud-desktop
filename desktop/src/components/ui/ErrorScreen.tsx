import { useTranslation } from 'react-i18next';
import { AlertCircle, Home, RefreshCw } from '../../lib/icons';

/** Premium crash screen shown by the ErrorBoundary. Dark, accent-lit glass,
 *  with retry / reload / home actions and a foldaway technical detail. */
export function ErrorScreen({
  error,
  onRetry,
  fullscreen = false,
}: {
  error?: Error | null;
  onRetry?: () => void;
  fullscreen?: boolean;
}) {
  const { t } = useTranslation();
  const message = error?.message || String(error ?? '');

  return (
    <div
      className={`relative flex items-center justify-center overflow-hidden p-6 ${
        fullscreen ? 'h-screen' : 'min-h-full'
      }`}
      style={{ background: fullscreen ? 'var(--bg-primary, #08080a)' : undefined }}
    >
      <div className="relative z-10 w-full max-w-[460px]">
        <div
          className="relative overflow-hidden rounded-[2rem] p-8 text-center"
          style={{
            border: '0.5px solid rgba(255,255,255,0.1)',
            background: '#141417',
          }}
        >
          {/* emblem */}
          <div className="relative mx-auto mb-6 flex h-20 w-20 items-center justify-center">
            <span
              className="relative flex h-20 w-20 items-center justify-center rounded-[26px]"
              style={{
                color: 'var(--color-accent)',
                background: 'rgba(255,255,255,0.05)',
                border: '0.5px solid rgba(255,255,255,0.1)',
              }}
            >
              <AlertCircle size={34} strokeWidth={1.8} />
            </span>
          </div>

          <h1 className="text-[26px] font-black tracking-tight leading-tight text-white">
            {t('errors.title')}
          </h1>
          <p className="mt-2.5 text-[13.5px] leading-relaxed text-white/45">
            {t('errors.subtitle')}
          </p>

          {message && (
            <details className="group mt-5 text-left">
              <summary className="cursor-pointer list-none text-[11px] font-medium text-white/35 transition-colors hover:text-white/60">
                <span className="text-[var(--color-accent)]">▸</span> {t('errors.details')}
              </summary>
              <pre className="mt-2 max-h-40 overflow-auto rounded-xl border border-white/[0.06] bg-black/40 p-3 text-[11px] leading-relaxed text-red-300/80 whitespace-pre-wrap break-words">
                {message}
              </pre>
            </details>
          )}

          <div className="mt-7 flex flex-col gap-2.5">
            {onRetry && (
              <button
                type="button"
                onClick={onRetry}
                className="group relative flex h-12 w-full items-center justify-center gap-2 overflow-hidden rounded-2xl text-sm font-bold transition-transform duration-200 hover:scale-[1.02] active:scale-[0.97] cursor-pointer"
                style={{
                  color: 'var(--color-accent-contrast)',
                  background: 'var(--color-accent)',
                }}
              >
                <RefreshCw size={16} strokeWidth={2.2} />
                {t('errors.retry')}
              </button>
            )}
            <div className="flex gap-2.5">
              <button
                type="button"
                onClick={() => window.location.reload()}
                className="flex h-11 flex-1 items-center justify-center gap-2 rounded-2xl border border-white/[0.1] bg-white/[0.04] text-[13px] font-semibold text-white/70 transition-all hover:bg-white/[0.08] hover:text-white/90 active:scale-[0.97] cursor-pointer"
              >
                <RefreshCw size={14} />
                {t('errors.reload')}
              </button>
              <button
                type="button"
                onClick={() => {
                  window.location.assign('/');
                }}
                className="flex h-11 flex-1 items-center justify-center gap-2 rounded-2xl border border-white/[0.1] bg-white/[0.04] text-[13px] font-semibold text-white/70 transition-all hover:bg-white/[0.08] hover:text-white/90 active:scale-[0.97] cursor-pointer"
              >
                <Home size={14} />
                {t('errors.home')}
              </button>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
