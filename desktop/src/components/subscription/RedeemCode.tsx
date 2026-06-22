import { useMutation } from '@tanstack/react-query';
import { memo, useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { PayError, payApi } from '../../lib/pay-client';
import { usePerfMode } from '../../lib/perf';
import { requestPremiumRecheck } from '../../lib/premium-cache';
import { GlassButton } from '../ui/GlassButton';

const CODE_RE = /^STAR(-[0-9A-Z]{4}){4}$/;

/** Strip to alphanumerics, drop a leading "STAR", cap to 16 body chars. */
function normalizeBody(raw: string): string {
  let s = raw.toUpperCase().replace(/[^0-9A-Z]/g, '');
  if (s.startsWith('STAR')) s = s.slice(4);
  return s.slice(0, 16);
}

/** body (≤16) → "STAR-XXXX-XXXX-XXXX-XXXX" with progressive dashes. */
function format(body: string): string {
  const groups = body.match(/.{1,4}/g) ?? [];
  return ['STAR', ...groups].join('-');
}

function redeemErrorKey(err: unknown): string {
  if (err instanceof PayError) {
    switch (err.status) {
      case 409:
        return 'starpass.redeem.errUsed';
      case 404:
        return 'starpass.redeem.errUnknown';
      case 403:
        return 'starpass.redeem.errRevoked';
      case 400:
        return 'starpass.redeem.errExpired';
    }
  }
  return 'starpass.redeem.errGeneric';
}

export const RedeemCode = memo(function RedeemCode({ onRedeemed }: { onRedeemed: () => void }) {
  const { t } = useTranslation();
  const perf = usePerfMode();
  const [body, setBody] = useState('');
  const code = useMemo(() => format(body), [body]);
  const valid = CODE_RE.test(code);

  const mutation = useMutation({
    mutationFn: () => payApi.redeem(code),
    onSuccess: () => {
      requestPremiumRecheck();
      onRedeemed();
    },
  });

  const onChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      setBody(normalizeBody(e.target.value));
      mutation.reset();
    },
    [mutation],
  );

  const onPaste = useCallback(async () => {
    try {
      const text = await navigator.clipboard.readText();
      setBody(normalizeBody(text));
      mutation.reset();
    } catch {
      /* clipboard denied — ignore */
    }
  }, [mutation]);

  return (
    <div className="grid grid-cols-1 items-center gap-10 lg:grid-cols-2">
      <div>
        <h2
          className="mb-3 text-[30px] font-medium tracking-[-0.01em]"
          style={{ fontFamily: 'var(--font-serif)' }}
        >
          {t('starpass.redeem.heading')}
        </h2>
        <p className="max-w-[360px] text-[14px] text-white/55">{t('starpass.redeem.lead')}</p>
        <div className="mt-[18px] flex flex-wrap gap-[14px]">
          <span className="rounded-full border border-white/[0.06] bg-white/[0.03] px-[11px] py-[6px] font-mono text-[11px] text-white/55">
            {t('starpass.redeem.format')}{' '}
            <b className="font-medium text-accent">STAR-XXXX-XXXX-XXXX-XXXX</b>
          </span>
          <span className="rounded-full border border-white/[0.06] bg-white/[0.03] px-[11px] py-[6px] font-mono text-[11px] text-white/55">
            {t('starpass.redeem.oneTime')}
          </span>
        </div>
      </div>

      <div
        className="rounded-2xl border border-white/[0.06] p-[26px]"
        style={{
          background: perf.blur(40) ? 'rgba(255,255,255,0.03)' : 'rgba(22,22,26,0.85)',
          backdropFilter: perf.blur(40) ? `blur(${perf.blur(40)}px)` : undefined,
          WebkitBackdropFilter: perf.blur(40) ? `blur(${perf.blur(40)}px)` : undefined,
        }}
      >
        <span className="mb-3 block font-mono text-[10.5px] uppercase tracking-[0.16em] text-white/35">
          {t('starpass.redeem.label')}
        </span>
        <input
          type="text"
          value={code}
          onChange={onChange}
          spellCheck={false}
          autoCapitalize="characters"
          aria-label={t('starpass.redeem.label')}
          placeholder="STAR-XXXX-XXXX-XXXX-XXXX"
          className="selectable w-full rounded-xl border bg-black/25 px-4 py-[14px] font-mono text-[20px] uppercase tracking-[0.06em] text-white/90 outline-none transition-all duration-200 placeholder:text-white/25 focus:border-accent"
          style={{
            borderColor: 'rgba(255,255,255,0.12)',
            boxShadow: valid
              ? '0 0 0 3px color-mix(in srgb, var(--color-accent) 18%, transparent)'
              : undefined,
          }}
        />
        <div className="mt-3 font-mono text-[12px] text-white/35">
          {t('starpass.redeem.progress', { count: body.length })}
        </div>

        {mutation.isError && (
          <div className="mt-[10px] flex items-start gap-2 text-[12.5px] text-red-300/95">
            <span className="text-red-400">✕</span>
            <span>{t(redeemErrorKey(mutation.error))}</span>
          </div>
        )}

        <div className="mt-[18px] flex flex-wrap items-center gap-3">
          <GlassButton
            variant="primary"
            disabled={!valid || mutation.isPending}
            onClick={() => mutation.mutate()}
          >
            {mutation.isPending ? t('starpass.redeem.activating') : t('starpass.redeem.activate')}
          </GlassButton>
          <GlassButton variant="ghost" onClick={onPaste}>
            {t('starpass.redeem.paste')}
          </GlassButton>
        </div>
      </div>
    </div>
  );
});
