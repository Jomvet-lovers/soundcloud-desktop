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
      className="group relative mt-5 w-full max-w-[440px] overflow-hidden rounded-[16px] p-px text-left transition-transform duration-300 ease-[var(--ease-apple)] hover:-translate-y-[2px]"
      style={{
        // Foil rim catching the user's accent, matching the plan cards / pass edge.
        background:
          'linear-gradient(120deg, color-mix(in srgb, var(--color-accent) 42%, rgba(255,255,255,0.14)), rgba(255,255,255,0.03) 58%, rgba(255,255,255,0.09))',
      }}
    >
      <div
        className="flex items-center gap-[15px] rounded-[15px] px-[18px] py-[15px]"
        style={{
          background:
            'linear-gradient(120deg, color-mix(in srgb, var(--color-accent) 11%, rgb(16,14,19)) 0%, rgb(14,13,16) 70%)',
        }}
      >
        <span
          className="grid size-10 shrink-0 place-items-center rounded-xl"
          style={{
            color: 'var(--color-accent-contrast)',
            background: 'var(--color-accent)',
            boxShadow: perf.glow ? '0 6px 18px -7px var(--color-accent-glow)' : undefined,
          }}
        >
          <BoltGlyph size={19} />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-[14.5px] font-medium text-white/95">
            {t('starpass.boosty.title')}
            <ExternalLink
              size={13}
              className="text-white/40 transition-transform duration-300 group-hover:translate-x-[1px] group-hover:-translate-y-[1px]"
            />
          </div>
          <p className="mt-[3px] text-[12.5px] leading-[1.42] text-white/45">
            {t('starpass.boosty.sub')}
          </p>
        </div>
      </div>
    </button>
  );
});
