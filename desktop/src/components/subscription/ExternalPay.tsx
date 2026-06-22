import { openUrl } from '@tauri-apps/plugin-opener';
import { memo, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink } from '../../lib/icons';
import type { CheckoutResp } from '../../lib/pay-client';
import { passSerial } from '../../lib/star-format';
import { GlassButton } from '../ui/GlassButton';
import { type PayPhase, PayStatus } from './PayStatus';
import { StarPass } from './StarPass';

interface ExternalPayProps {
  checkout: CheckoutResp;
  handle: string;
  tier: string;
  methodLabel: string;
  phase: PayPhase;
  onChangeMethod: () => void;
}

export const ExternalPay = memo(function ExternalPay({
  checkout,
  handle,
  tier,
  methodLabel,
  phase,
  onChangeMethod,
}: ExternalPayProps) {
  const { t } = useTranslation();
  const serial = passSerial(checkout.order_id);
  const amount = `${checkout.amount_rub} ₽`;
  const targets = checkout.pay_targets ?? [];

  // Auto-open the provider checkout once when the URL arrives.
  const openedRef = useRef(false);
  const open = useCallback(() => {
    if (checkout.pay_url) void openUrl(checkout.pay_url).catch(() => {});
  }, [checkout.pay_url]);

  useEffect(() => {
    if (!openedRef.current && checkout.pay_url) {
      openedRef.current = true;
      open();
    }
  }, [checkout.pay_url, open]);

  return (
    <div className="grid grid-cols-1 items-stretch gap-7 lg:grid-cols-[1.1fr_0.9fr]">
      <StarPass
        variant="boarding"
        handle={handle}
        caption={t('starpass.caption.boarding')}
        tier={tier}
        fields={[
          { label: t('starpass.member'), value: handle },
          { label: t('starpass.fieldMethod'), value: methodLabel },
          { label: t('starpass.fieldOrder'), value: serial },
        ]}
      />

      <div className="flex flex-col gap-[18px]">
        <div
          className="rounded-2xl border border-white/[0.06] px-[22px] py-5"
          style={{ background: 'rgba(255,255,255,0.03)' }}
        >
          <span className="mb-2 block font-mono text-[10.5px] uppercase tracking-[0.16em] text-white/35">
            {t('starpass.toPay')}
          </span>
          <div className="font-mono text-[34px] tabular-nums tracking-[-0.01em]">{amount}</div>
          <div className="mt-1 text-[13px] text-white/55">
            {tier} · {methodLabel}
          </div>
        </div>

        <PayStatus phase={phase} />

        <p className="text-[13px] leading-relaxed text-white/55">{t('starpass.externalHint')}</p>

        {targets.length > 1 ? (
          <div className="flex flex-col gap-2">
            <span className="font-mono text-[10.5px] uppercase tracking-[0.16em] text-white/35">
              {t('starpass.openVia')}
            </span>
            <div className="flex flex-wrap gap-2">
              {targets.map((tg) => (
                <GlassButton
                  key={tg.kind}
                  variant={tg.kind === 'tg' ? 'primary' : 'ghost'}
                  onClick={() => void openUrl(tg.url).catch(() => {})}
                >
                  {t(`starpass.openIn.${tg.kind}`)}
                  <ExternalLink size={13} />
                </GlassButton>
              ))}
            </div>
          </div>
        ) : (
          <GlassButton variant="primary" onClick={open} className="self-start">
            {t('starpass.openCheckout')}
            <ExternalLink size={14} />
          </GlassButton>
        )}

        <GlassButton variant="ghost" className="self-start" onClick={onChangeMethod}>
          {t('starpass.changeMethod')}
        </GlassButton>
      </div>
    </div>
  );
});
