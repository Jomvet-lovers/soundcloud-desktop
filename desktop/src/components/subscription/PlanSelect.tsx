import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import type { Plan } from '../../lib/pay-client';
import { usePerfMode } from '../../lib/perf';
import { FOIL_GRADIENT } from './foil';

interface PlanSelectProps {
  plans: Plan[];
  selectedId: string | null;
  onSelect: (id: string) => void;
}

function planClass(months: number): string {
  if (months >= 12) return 'S';
  if (months >= 3) return 'B';
  return 'A';
}

const PlanCard = memo(function PlanCard({
  plan,
  selected,
  best,
  onSelect,
}: {
  plan: Plan;
  selected: boolean;
  best: boolean;
  onSelect: () => void;
}) {
  const { t } = useTranslation();
  const perf = usePerfMode();
  const idle = perf.idleAnim;
  const perMonth = Math.round(plan.price_rub / plan.months);

  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={onSelect}
      className="relative cursor-pointer rounded-[18px] p-px text-left transition-transform duration-300 ease-[var(--ease-apple)] hover:-translate-y-[3px]"
      style={{
        background: selected
          ? FOIL_GRADIENT
          : 'linear-gradient(160deg, rgba(255,255,255,0.10), rgba(255,255,255,0.02))',
        backgroundSize: selected ? '200%' : undefined,
        animation: selected && idle ? 'star-foil-sweep 10s linear infinite' : undefined,
      }}
    >
      {best && (
        <span
          className="absolute right-[18px] top-px z-[2] -translate-y-1/2 rounded-full px-[10px] py-1 font-mono text-[10px] uppercase tracking-[0.14em] text-accent-contrast"
          style={{
            background: 'var(--color-accent)',
            boxShadow: '0 8px 20px -8px color-mix(in srgb, var(--color-accent) 80%, transparent)',
          }}
        >
          {t('starpass.bestValue')}
        </span>
      )}
      <div
        className="h-full rounded-[17px] px-[22px] pb-[22px] pt-6"
        style={{
          background: selected
            ? 'linear-gradient(160deg, rgba(30,21,16,0.94), rgba(15,12,13,0.95))'
            : 'linear-gradient(160deg, rgba(20,19,24,0.9), rgba(13,13,16,0.92))',
        }}
      >
        <div className="mb-4 flex items-center justify-between">
          <div>
            <div className="text-[19px] font-medium" style={{ fontFamily: 'var(--font-serif)' }}>
              {t(
                `starpass.plan.${plan.months >= 12 ? 'year' : plan.months >= 3 ? 'quarter' : 'month'}`,
              )}
            </div>
            <div className="font-mono text-[11px] tracking-[0.1em] text-white/35">
              {plan.months}
              {' · '}
              {t('starpass.classLabel', { cls: planClass(plan.months) })}
            </div>
          </div>
          <span
            className="relative size-4 shrink-0 rounded-full"
            style={{
              border: selected
                ? '1.5px solid var(--color-accent)'
                : '1.5px solid rgba(255,255,255,0.12)',
            }}
          >
            {selected && (
              <span
                className="absolute rounded-full"
                style={{ inset: 3, background: 'var(--color-accent)' }}
              />
            )}
          </span>
        </div>

        <div className="mb-[2px] mt-[6px] flex items-baseline gap-2">
          <span className="font-mono text-[32px] tabular-nums tracking-[-0.01em]">
            {plan.price_rub}
          </span>
          <span className="font-mono text-[18px] text-white/55">₽</span>
        </div>

        <div className="mb-[14px] text-[13px] text-white/55">
          <b className="font-mono font-medium text-white/90">{perMonth} ₽</b>
          {t('starpass.perMonth')}
          {plan.savings_pct > 0 && (
            <span
              className="ml-2 inline-flex items-center rounded-[7px] px-2 py-[2px] font-mono text-[11px] tracking-[0.06em] text-accent"
              style={{
                border: '1px solid color-mix(in srgb, var(--color-accent) 38%, transparent)',
              }}
            >
              −{plan.savings_pct}%
            </span>
          )}
        </div>

        <div className="my-[14px] h-px bg-white/[0.06]" />

        <div className="flex justify-between py-[3px] text-[12.5px] text-white/55">
          <span>{t('starpass.starsLabel')}</span>
          <span className="font-mono tabular-nums text-white/90">~{plan.stars} ★</span>
        </div>
      </div>
    </button>
  );
});

export const PlanSelect = memo(function PlanSelect({
  plans,
  selectedId,
  onSelect,
}: PlanSelectProps) {
  const bestId = plans.reduce<Plan | null>(
    (best, p) => (!best || p.savings_pct > best.savings_pct ? p : best),
    null,
  )?.id;
  return (
    <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
      {plans.map((p) => (
        <PlanCard
          key={p.id}
          plan={p}
          selected={p.id === selectedId}
          best={p.id === bestId}
          onSelect={() => onSelect(p.id)}
        />
      ))}
    </div>
  );
});
