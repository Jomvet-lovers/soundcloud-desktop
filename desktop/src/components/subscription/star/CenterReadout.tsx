import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { Check, Loader2 } from '../../../lib/icons';
import type { Plan } from '../../../lib/pay-client';
import { daysUntil, passDate, passSerial } from '../../../lib/star-format';
import type { PayPhase } from '../PayStatus';
import { monthsKey, type Step } from './meta';

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
  const serif = { fontFamily: 'var(--font-serif)' };

  if (step === 'success' || step === 'manage') {
    return (
      <div>
        <div className="text-[30px] font-medium leading-none text-white" style={serif}>
          {handle}
        </div>
        <div className="mt-2.5 font-mono text-[11.5px] tracking-[0.16em] text-accent">
          {passSerial(serialSeed)}
        </div>
        <div className="mt-1.5 font-mono text-[11px] tracking-[0.1em] text-white/55">
          {t('starpass.until')} {passDate(endsAt)} ·{' '}
          {t('starpass.daysLeft', { count: daysUntil(endsAt) })}
        </div>
      </div>
    );
  }

  if (step === 'pay') {
    const amount = plan ? `${plan.price_rub} ₽` : '';
    return (
      <div>
        <div className="text-[40px] font-medium leading-none text-white" style={serif}>
          {amount}
        </div>
        <div className="mt-3 inline-flex items-center gap-2 font-mono text-[11.5px] tracking-[0.1em] text-white/60">
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
      <div>
        <div className="text-[24px] text-accent">✦</div>
        <div className="mt-2 font-mono text-[12px] tracking-[0.08em] text-white/60">
          {t('starpass.redeem.center')}
        </div>
      </div>
    );
  }

  // overview / method
  const amount = plan ? `${plan.price_rub}` : '—';
  const perMonth = plan ? Math.round(plan.price_rub / plan.months) : 0;
  return (
    <div>
      <div className="text-[18px] text-accent">✦</div>
      <div className="mt-2 text-[52px] font-medium leading-[0.95] text-white" style={serif}>
        {amount}
        <span className="text-[22px] text-white/55"> ₽</span>
      </div>
      <div className="mt-3 font-mono text-[12px] tracking-[0.08em] text-white/60">
        {plan ? (
          <>
            {t(`starpass.plan.${monthsKey(plan.months)}`)} · {perMonth} ₽/
            {t('starpass.perMonthShort')}
            {plan.savings_pct > 0 && <span className="text-accent"> · −{plan.savings_pct}%</span>}
          </>
        ) : (
          t('starpass.loading')
        )}
      </div>
    </div>
  );
});
