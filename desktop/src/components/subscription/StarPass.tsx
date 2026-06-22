import React, { type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { usePerfMode } from '../../lib/perf';
import { FOIL_GRADIENT } from './foil';

export type StarPassVariant = 'hero' | 'boarding' | 'activated' | 'manage';

export interface StarPassField {
  label: string;
  value: string;
  big?: boolean;
}

interface StarPassProps {
  variant: StarPassVariant;
  /** Member handle, e.g. "@nightset". */
  handle?: string;
  /** Small text under the ★ STAR wordmark (e.g. "Member pass · active"). */
  caption: string;
  /** Right-hand chip (tier / class). */
  tier: string;
  /** Mono field grid below the rule. */
  fields: StarPassField[];
  /** Custom stub content (QR for boarding); defaults to medallion + barcode. */
  stub?: ReactNode;
  /** Force the stub below the body (QR boarding) instead of the responsive split. */
  stubBelow?: boolean;
  /** "Activated" rotated foil stamp (success). */
  stamped?: boolean;
}

/* ── default stub: foil medallion + decorative barcode ─────────── */
function DefaultStub({ label, idle }: { label: string; idle: boolean }) {
  return (
    <>
      <div
        className="grid size-14 shrink-0 place-items-center rounded-full"
        style={{
          background: FOIL_GRADIENT,
          backgroundSize: '200% 200%',
          animation: idle ? 'star-foil-text 8s linear infinite' : undefined,
          boxShadow: '0 8px 24px -8px rgba(0,0,0,0.7), inset 0 0 0 1px rgba(255,255,255,0.2)',
        }}
      >
        <span className="text-[24px] font-bold leading-none text-black/60">★</span>
      </div>
      <div
        className="h-[42px] w-full min-w-[80px] rounded-md opacity-80"
        style={{
          background:
            'repeating-linear-gradient(90deg,#e9e9ec 0 2px, transparent 2px 4px, #e9e9ec 4px 5px, transparent 5px 9px)',
        }}
      />
      <div className="text-center font-mono text-[10.5px] tracking-[0.12em] text-white/55">
        {label}
      </div>
    </>
  );
}

/**
 * STAR membership pass — the page's concept artifact. Foil rim + holographic
 * sweep over near-black glass, a perforated tear-off stub (medallion + barcode),
 * and a mono serial/validity grid. Built on a CSS container so the stub drops
 * below the body on narrow shells (split-view / vertical monitors) and sits
 * beside it on wide ones — no fixed two-column layout that overflows.
 */
export const StarPass = React.memo(function StarPass({
  variant,
  handle,
  caption,
  tier,
  fields,
  stub,
  stubBelow = false,
  stamped = false,
}: StarPassProps) {
  const { t } = useTranslation();
  const perf = usePerfMode();
  const idle = perf.idleAnim;
  const bodyBlur = perf.blur(40);

  // Foil intensity per variant: manage is calmest, activated brightest.
  const foilOpacity =
    variant === 'activated'
      ? 0.3
      : variant === 'manage'
        ? 0.12
        : variant === 'boarding'
          ? 0.1
          : 0.16;

  // Responsive split: stub beside the body on wide containers, below on narrow.
  // `stubBelow` (QR boarding) pins it below at every width.
  const gridCls = stubBelow
    ? 'grid-cols-1'
    : 'grid-cols-1 @[460px]:grid-cols-[1fr_minmax(160px,196px)]';
  const stubCls = stubBelow
    ? 'flex-row border-t'
    : 'flex-row border-t @[460px]:flex-col @[460px]:border-t-0 @[460px]:border-l';

  return (
    <div
      className="relative w-full rounded-[22px] p-px"
      style={{
        isolation: 'isolate',
        // Edge catches the user's accent (foil rim), not a fixed warm tone.
        background:
          'linear-gradient(150deg, color-mix(in srgb, var(--color-accent) 32%, rgba(255,255,255,0.14)), rgba(255,255,255,0.02) 42%, rgba(255,255,255,0.10))',
        boxShadow: perf.glow
          ? '0 40px 90px -30px rgba(0,0,0,0.8), 0 0 72px -24px color-mix(in srgb, var(--color-accent) 60%, transparent), inset 0 2px 0 rgba(255,255,255,0.05)'
          : '0 40px 90px -30px rgba(0,0,0,0.8), inset 0 2px 0 rgba(255,255,255,0.05)',
      }}
    >
      <div
        className="@container relative overflow-hidden rounded-[21px]"
        style={{
          // Dark surface washed with the user's accent (not a fixed warm tone), so
          // the pass takes on their colour while staying near-black and premium.
          background: bodyBlur
            ? 'linear-gradient(160deg, color-mix(in srgb, var(--color-accent) 16%, rgb(13,12,16)) 0%, rgb(13,12,16) 60%)'
            : 'linear-gradient(160deg, color-mix(in srgb, var(--color-accent) 13%, rgb(15,14,18)) 0%, rgb(15,14,18) 60%)',
          backdropFilter: bodyBlur ? `blur(${bodyBlur}px)` : undefined,
          WebkitBackdropFilter: bodyBlur ? `blur(${bodyBlur}px)` : undefined,
        }}
      >
        {/* holographic foil sweep (warm metal, screen blend, accent-anchored) */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 z-0"
          style={{
            background: FOIL_GRADIENT,
            backgroundSize: '300% 300%',
            opacity: foilOpacity,
            mixBlendMode: 'screen',
            filter: 'saturate(140%)',
            animation: idle ? 'star-foil-sweep 14s linear infinite' : undefined,
            backgroundPosition: idle ? undefined : '40% 50%',
          }}
        />
        {/* guilloché engraving */}
        <div
          aria-hidden
          className="pointer-events-none absolute inset-0 z-0 opacity-50"
          style={{
            backgroundImage:
              'repeating-linear-gradient(115deg, rgba(255,255,255,0.025) 0 1px, transparent 1px 9px)',
            WebkitMaskImage: 'radial-gradient(120% 120% at 80% 10%, #000, transparent 70%)',
            maskImage: 'radial-gradient(120% 120% at 80% 10%, #000, transparent 70%)',
          }}
        />

        {/* "Activated" foil stamp */}
        {stamped && (
          <span
            className="absolute right-[18px] top-[18px] z-[5] rounded-full px-[11px] py-[5px] font-mono text-[10px] uppercase tracking-[0.16em] text-accent-contrast"
            style={{
              background: 'color-mix(in srgb, var(--color-accent) 88%, #000)',
              transform: 'rotate(-4deg)',
              boxShadow: '0 8px 22px -8px color-mix(in srgb, var(--color-accent) 80%, transparent)',
              animation: idle ? 'star-stamp-in 0.7s var(--ease-apple) both' : undefined,
            }}
          >
            {t('starpass.activatedStamp')}
          </span>
        )}

        <div className={`relative z-[2] grid ${gridCls}`} style={{ isolation: 'isolate' }}>
          {/* main */}
          <div className="min-w-0 px-6 pb-6 pt-7 @[460px]:px-[30px] @[460px]:pb-[26px] @[460px]:pt-[30px]">
            <div className="flex items-start justify-between gap-3">
              <div className="flex min-w-0 flex-col gap-[3px]">
                <div
                  className="font-serif text-[26px] font-semibold leading-none tracking-[0.02em] @[460px]:text-[30px]"
                  style={{ fontFamily: 'var(--font-serif)' }}
                >
                  <span
                    style={{
                      background: FOIL_GRADIENT,
                      backgroundSize: '200%',
                      WebkitBackgroundClip: 'text',
                      backgroundClip: 'text',
                      color: 'transparent',
                      animation: idle ? 'star-foil-text 9s linear infinite' : undefined,
                    }}
                  >
                    ★
                  </span>{' '}
                  STAR
                </div>
                <span className="truncate font-mono text-[10.5px] uppercase tracking-[0.16em] text-white/35">
                  {caption}
                </span>
              </div>
              <span
                className="whitespace-nowrap rounded-full px-[9px] py-[5px] font-mono text-[10px] uppercase tracking-[0.16em] text-white/90"
                style={{
                  border: '1px solid color-mix(in srgb, var(--color-accent) 45%, transparent)',
                  background: 'color-mix(in srgb, var(--color-accent) 14%, transparent)',
                }}
              >
                {tier}
              </span>
            </div>

            <div className="my-[22px] h-px bg-white/[0.12]" />

            {handle && (
              <div className="mb-[18px]">
                <span className="mb-[5px] block font-mono text-[10.5px] uppercase tracking-[0.16em] text-white/35">
                  {t('starpass.member')}
                </span>
                <span
                  className="block truncate text-[22px] font-medium tracking-[0.01em]"
                  style={{ fontFamily: 'var(--font-serif)' }}
                >
                  {handle}
                </span>
              </div>
            )}

            <div className="grid grid-cols-2 gap-x-5 gap-y-[18px] @[460px]:grid-cols-3">
              {fields.map((f) => (
                <div key={f.label} className="min-w-0">
                  <span className="mb-[5px] block font-mono text-[10.5px] uppercase tracking-[0.16em] text-white/35">
                    {f.label}
                  </span>
                  <span
                    className={`block truncate font-mono tabular-nums tracking-[0.01em] text-white/90 ${
                      f.big ? 'text-[16px]' : 'text-[14px]'
                    }`}
                  >
                    {f.value}
                  </span>
                </div>
              ))}
            </div>
          </div>

          {/* stub (perforated tear-off) */}
          <div
            className={`relative z-[2] flex items-center justify-between gap-[14px] border-dashed border-white/[0.18] p-6 @[460px]:items-stretch ${stubCls}`}
            style={{ background: 'rgba(255,255,255,0.018)' }}
          >
            {stub ?? <DefaultStub label={t('starpass.allAccess')} idle={idle} />}
          </div>
        </div>
      </div>
    </div>
  );
});
