import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Loader2 } from '../../../lib/icons';
import type { Plan } from '../../../lib/pay-client';
import { daysUntil, passDate, passSerial } from '../../../lib/star-format';
import type { PayPhase } from '../PayStatus';
import { monthsKey, type Step } from './meta';

// Drop-shadow so text stays crisp over the core's bloom (the dark well alone
// isn't enough for the muted meta line).
const SHADOW = { textShadow: '0 2px 20px rgba(0,0,0,0.95), 0 0 3px rgba(0,0,0,0.7)' } as const;
const SERIF = { fontFamily: 'var(--font-serif)', ...SHADOW } as const;

/** The content that lives inside the living core's dark "well", per flow state. */
export const CenterReadout = memo(function CenterReadout({
  step,
  phase,
  handle,
  plan,
  endsAt,
  serialSeed,
}: {
  step: Step;
  phase: PayPhase;
  handle: string;
  plan: Plan | null;
  endsAt: number;
  serialSeed: string;
}) {
  const { t } = useTranslation();

  if (step === 'success' || step === 'manage') {
    return (
      <div style={SHADOW}>
        <div className="text-[30px] font-medium leading-none text-white" style={SERIF}>
          {handle}
        </div>
        <div className="mt-2.5 font-mono text-[11.5px] tracking-[0.16em] text-accent">
          {passSerial(serialSeed)}
        </div>
        <div className="mt-1.5 font-mono text-[11px] tracking-[0.1em] text-white/70">
          {t('starpass.until')} {passDate(endsAt)} ·{' '}
          {t('starpass.daysLeft', { count: daysUntil(endsAt) })}
        </div>
      </div>
    );
  }

  if (step === 'pay') {
    const amount = plan ? `${plan.price_rub} ₽` : '';
    return (
      <div style={SHADOW}>
        <div className="text-[40px] font-medium leading-none text-white" style={SERIF}>
          {amount}
        </div>
        <div className="mt-3 inline-flex items-center gap-2 text-[12px] font-medium tracking-[0.02em] text-white/75">
          {phase === 'granted' ? (
            <>
              <span className="grid size-[18px] place-items-center rounded-full bg-accent text-[11px] font-bold text-accent-contrast">
                <Check size={11} strokeWidth={3} />
              </span>
              {t('starpass.status.paid')}
            </>
          ) : phase === 'failed' ? (
            <span className="text-red-300/90">{t('starpass.status.failed')}</span>
          ) : (
            <>
              <Loader2 size={15} className="animate-spin text-accent" />{' '}
              {t('starpass.status.waiting')}
            </>
          )}
        </div>
      </div>
    );
  }

  if (step === 'redeem') {
    return (
      <div style={SHADOW}>
        <div className="text-[24px] text-accent">✦</div>
        <div className="mt-2 text-[12.5px] font-medium tracking-[0.02em] text-white/70">
          {t('starpass.redeem.center')}
        </div>
      </div>
    );
  }

  // overview / method
  const amount = plan ? `${plan.price_rub}` : '—';
  const perMonth = plan ? Math.round(plan.price_rub / plan.months) : 0;
  return (
    <div style={SHADOW}>
      <div className="text-[18px] text-accent">✦</div>
      <div className="mt-2 text-[52px] font-medium leading-[0.95] text-white" style={SERIF}>
        {amount}
        <span className="text-[22px] text-white/55"> ₽</span>
      </div>
      <div className="mt-3 text-[13px] font-medium tracking-[0.01em] text-white/75">
        {plan ? (
          <>
            {t(`starpass.plan.${monthsKey(plan.months)}`)} · {perMonth} ₽/
            {t('starpass.perMonthShort')}
            {plan.savings_pct > 0 && (
              <span className="font-semibold text-accent"> · −{plan.savings_pct}%</span>
            )}
          </>
        ) : (
          t('starpass.loading')
        )}
      </div>
    </div>
  );
});
