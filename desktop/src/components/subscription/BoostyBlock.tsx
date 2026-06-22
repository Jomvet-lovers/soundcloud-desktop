import { openUrl } from '@tauri-apps/plugin-opener';
import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink } from '../../lib/icons';
import { usePerfMode } from '../../lib/perf';

// Direct Boosty subscribe page. Boosty is an external flow: subscribe there → boost
// role in our Discord → the bot grants STAR (after /sc-link). Not an in-app provider.
const BOOSTY_URL = 'https://boosty.to/lolinamide/purchase/3886747';

/* Boosty lightning mark, drawn in accent-contrast ink on the accent tile (so it
 * stays legible on any accent — light or dark). */
const BoltGlyph = memo(function BoltGlyph({ size = 19 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path d="M13.6 2 4.5 13.2a.6.6 0 0 0 .47.98H10l-1.2 7.06a.4.4 0 0 0 .72.3l9.06-11.2a.6.6 0 0 0-.47-.98H13l1.3-6.96a.4.4 0 0 0-.7-.4Z" />
    </svg>
  );
});

/** "Subscribe via Boosty" block on the STAR hero — opens the Boosty page in the
 * browser. Styled to the pass language (foil rim, accent tile), accent-aware. */
export const BoostyBlock = memo(function BoostyBlock() {
  const { t } = useTranslation();
  const perf = usePerfMode();
  const open = useCallback(() => {
    void openUrl(BOOSTY_URL).catch(() => {});
  }, []);

  return (
    <button
      type="button"
      onClick={open}
      className="group mt-5 flex w-full max-w-[420px] items-center gap-3 rounded-[13px] border border-white/[0.07] px-3.5 py-2.5 text-left transition-colors duration-200 hover:border-white/[0.14] hover:bg-white/[0.03]"
    >
      <span
        className="grid size-8 shrink-0 place-items-center rounded-[9px]"
        style={{
          color: 'var(--color-accent-contrast)',
          background: 'var(--color-accent)',
          boxShadow: perf.glow ? '0 6px 18px -8px var(--color-accent-glow)' : undefined,
        }}
      >
        <BoltGlyph size={16} />
      </span>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13.5px] font-medium text-white/90">
          {t('starpass.boosty.title')}
        </div>
        <p className="truncate text-[11.5px] text-white/40">{t('starpass.boosty.sub')}</p>
      </div>
      <ExternalLink
        size={13}
        className="shrink-0 text-white/35 transition-transform duration-200 group-hover:translate-x-[1px] group-hover:-translate-y-[1px]"
      />
    </button>
  );
});
