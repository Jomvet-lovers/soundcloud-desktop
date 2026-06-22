import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import type { CheckoutResp } from '../../lib/pay-client';
import { passSerial } from '../../lib/star-format';
import { QrCode } from '../auth/QrCode';
import { GlassButton } from '../ui/GlassButton';
import { type PayPhase, PayStatus } from './PayStatus';
import { StarPass } from './StarPass';

interface SbpQrPayProps {
  checkout: CheckoutResp;
  handle: string;
  tier: string;
  phase: PayPhase;
  onChangeMethod: () => void;
}

export const SbpQrPay = memo(function SbpQrPay({
  checkout,
  handle,
  tier,
  phase,
  onChangeMethod,
}: SbpQrPayProps) {
  const { t } = useTranslation();
  const serial = passSerial(checkout.order_id);
  const amount = `${checkout.amount_rub} ₽`;

  const qrStub = (
    <div className="flex w-full flex-col items-center gap-3">
      {checkout.sbp_qr ? (
        <QrCode payload={checkout.sbp_qr} size={188} />
      ) : (
        <div className="grid size-[188px] place-items-center rounded-2xl bg-white/5 text-white/40">
          —
        </div>
      )}
      <div className="text-center">
        <div className="font-mono text-[20px] tabular-nums tracking-[0.02em]">{amount}</div>
        <div className="font-mono text-[11px] tracking-[0.12em] text-white/55">
          {t('starpass.method.sbp.title')} · NSPK · {serial}
        </div>
      </div>
    </div>
  );

  return (
    <div className="grid grid-cols-1 items-stretch gap-7 lg:grid-cols-[1.1fr_0.9fr]">
      <StarPass
        variant="boarding"
        handle={handle}
        caption={t('starpass.caption.boarding')}
        tier={tier}
        stubBelow
        fields={[
          { label: t('starpass.member'), value: handle },
          { label: t('starpass.fieldMethod'), value: t('starpass.method.sbp.title') },
          { label: t('starpass.fieldOrder'), value: serial },
        ]}
        stub={qrStub}
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
            {tier} · {handle}
          </div>
        </div>

        <PayStatus phase={phase} />

        <ol className="flex list-none flex-col gap-[10px] p-0">
          {[1, 2, 3].map((n) => (
            <li key={n} className="flex gap-[10px] text-[13px] text-white/55">
              <span className="min-w-[18px] font-mono text-[11px] text-accent">
                {String(n).padStart(2, '0')}
              </span>
              <span>{t(`starpass.sbpStep.${n}`)}</span>
            </li>
          ))}
        </ol>

        <GlassButton variant="ghost" className="self-start" onClick={onChangeMethod}>
          {t('starpass.changeMethod')}
        </GlassButton>
      </div>
    </div>
  );
});
