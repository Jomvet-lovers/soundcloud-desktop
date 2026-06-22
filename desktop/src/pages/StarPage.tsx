import { useMutation, useQuery } from '@tanstack/react-query';
import { memo, useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { CORE_CENTER_Y, LivingCore } from '../components/subscription/LivingCore';
import { phaseOf } from '../components/subscription/PayStatus';
import {
  type ActivationOption,
  activationOptions,
  toCheckout,
} from '../components/subscription/providers';
import { StarAtmosphere } from '../components/subscription/StarAtmosphere';
import { CenterReadout } from '../components/subscription/star/CenterReadout';
import { ManagePane } from '../components/subscription/star/ManagePane';
import { MethodPane } from '../components/subscription/star/MethodPane';
import { primaryEntitlement, type Step } from '../components/subscription/star/meta';
import { OverviewPane } from '../components/subscription/star/OverviewPane';
import { PayPane } from '../components/subscription/star/PayPane';
import { RedeemPane } from '../components/subscription/star/RedeemPane';
import { Console } from '../components/subscription/star/StarConsole';
import { SuccessPane } from '../components/subscription/star/SuccessPane';
import { ChevronLeft } from '../lib/icons';
import { type CheckoutResp, type Plan, payApi } from '../lib/pay-client';
import { usePremium } from '../lib/premium-cache';
import { useOrderPoll } from '../lib/useOrderPoll';
import { useAuthStore } from '../stores/auth';

/**
 * STAR membership — the "living core" instrument. Energy lives in the canvas
 * core; per-state content rides a floating glass console over it. This file is
 * pure orchestration (flow state machine + data wiring); every state's UI lives
 * in `components/subscription/star/*`.
 */
export const StarPage = memo(function StarPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const premium = usePremium();
  const username = useAuthStore((s) => s.user?.username);
  const handle = username ? `@${username}` : t('starpass.guestHandle');

  const [step, setStep] = useState<Step>(premium ? 'manage' : 'overview');
  const [planId, setPlanId] = useState<string | null>(null);
  const [option, setOption] = useState<ActivationOption | null>(null);
  const [recurring, setRecurring] = useState(false);
  const [checkout, setCheckout] = useState<CheckoutResp | null>(null);
  const [igniteKey, setIgniteKey] = useState(0);

  const plansQuery = useQuery({
    queryKey: ['pay', 'plans'],
    queryFn: payApi.plans,
    staleTime: 300_000,
  });
  const plans = plansQuery.data?.plans ?? [];
  const options = useMemo(
    () => (plansQuery.data ? activationOptions(plansQuery.data) : []),
    [plansQuery.data],
  );
  const selectedPlan = plans.find((p) => p.id === planId) ?? null;

  // auto-pick the best-value plan once plans arrive
  const bestId = plans.reduce<Plan | null>(
    (b, p) => (!b || p.savings_pct > b.savings_pct ? p : b),
    null,
  )?.id;
  if (!planId && bestId) setPlanId(bestId);

  const order = useOrderPoll(
    step === 'pay' || step === 'success' ? (checkout?.order_id ?? null) : null,
  );
  const phase = phaseOf(order.data?.status);
  if (step === 'pay' && phase === 'granted') {
    setIgniteKey((k) => k + 1);
    setStep('success');
  }

  // Active-membership end date (manage/success centre readout) — comes from the
  // subscription, not the in-flight order. Shared cache with ManagePane.
  const subQuery = useQuery({
    queryKey: ['pay', 'subscription'],
    queryFn: payApi.subscription,
    staleTime: 30_000,
    enabled: premium || step === 'manage' || step === 'success',
  });
  const subEndsAt = subQuery.data
    ? (primaryEntitlement(subQuery.data.entitlements)?.ends_at ?? subQuery.data.premium_until ?? 0)
    : 0;
  const centerEndsAt = step === 'manage' ? subEndsAt : (order.data?.premium_until ?? subEndsAt);

  const checkoutMut = useMutation({
    mutationFn: () => {
      if (!option || !planId) throw new Error('no selection');
      const allowRecurring = option.recurring && selectedPlan?.months === 1 && recurring;
      return payApi.checkout(toCheckout(option, planId, allowRecurring));
    },
    onSuccess: (resp) => {
      setCheckout(resp);
      setStep('pay');
    },
  });

  const reset = useCallback(() => {
    setCheckout(null);
    checkoutMut.reset();
  }, [checkoutMut]);

  const goMethod = useCallback(() => {
    reset();
    setStep('method');
  }, [reset]);

  const canRecur = !!option?.recurring && selectedPlan?.months === 1;
  const lit = step === 'success' || step === 'manage';
  // Where the header "back" returns to, per step (null = no back button).
  const backTarget: Step | null =
    step === 'pay'
      ? 'method'
      : step === 'method'
        ? 'overview'
        : step === 'redeem'
          ? premium
            ? 'manage'
            : 'overview'
          : step === 'overview' && premium
            ? 'manage'
            : null;
  // Active membership = the core fully open (max charge), like the 12-mo plan —
  // regardless of which plan is selected behind the scenes.
  const charge = lit
    ? 1
    : selectedPlan
      ? selectedPlan.months >= 12
        ? 1
        : selectedPlan.months >= 3
          ? 0.6
          : 0.28
      : 0.5;

  return (
    <div className="relative min-h-full w-full">
      <StarAtmosphere />
      <div
        className="relative z-10 mx-auto flex min-h-[calc(100vh-120px)] w-full max-w-[1060px] flex-col px-4 md:px-8"
        style={
          {
            isolation: 'isolate',
            // Scope the editorial display pair to /star only (no app-wide blast):
            // every `font-mono` / `var(--font-serif)` inside inherits these.
            '--font-serif': 'var(--font-display)',
            '--font-mono': 'var(--font-mono-console)',
          } as React.CSSProperties
        }
      >
        {/* frozen header — pinned so you can always go back / change the term */}
        <div className="sticky top-0 z-30 flex items-center justify-between py-4">
          <div className="flex items-center gap-2.5 font-mono text-[12px] uppercase tracking-[0.34em] text-white/60">
            <span className="text-accent">✦</span> STAR
          </div>
          {backTarget && (
            <button
              type="button"
              onClick={() => backTarget && setStep(backTarget)}
              className="flex cursor-pointer items-center gap-1.5 font-mono text-[11px] tracking-[0.14em] text-white/45 transition-colors hover:text-white/90"
            >
              <ChevronLeft size={14} /> {t('starpass.back')}
            </button>
          )}
        </div>

        {/* core region — flex-1 so the star centres and shrinks with the viewport */}
        <div className="relative flex min-h-[240px] flex-1">
          {/* living core backdrop */}
          <LivingCore
            charge={charge}
            waiting={step === 'pay' && phase === 'waiting'}
            lit={lit}
            igniteKey={igniteKey}
          />

          {/* aperture centre readout — dark lens scrim keeps it readable even when
              the lit core is at its brightest */}
          <div
            className="pointer-events-none absolute left-1/2 z-20 -translate-x-1/2 -translate-y-1/2"
            style={{ top: `${CORE_CENTER_Y * 100}%` }}
          >
            <div className="relative px-10 py-8 text-center">
              {/* single clean dark lens — darker core, smooth feather, sized a hair
                  larger than the text so glyphs never sit on the blurry edge (kills
                  the doubled-scrim smear; the canvas well underneath stays subtle). */}
              <div
                aria-hidden
                className="absolute -inset-x-8 -inset-y-5"
                style={{
                  background:
                    'radial-gradient(58% 54% at 50% 50%, rgba(3,3,5,0.95) 0%, rgba(3,3,5,0.86) 40%, rgba(3,3,5,0.42) 68%, transparent 86%)',
                }}
              />
              <div className="relative mx-auto max-w-[320px]">
                <CenterReadout
                  step={step}
                  phase={phase}
                  handle={handle}
                  plan={selectedPlan}
                  endsAt={centerEndsAt}
                  serialSeed={checkout?.order_id ?? handle}
                />
              </div>
            </div>
          </div>
        </div>

        {/* console — in normal flow beneath the core, can never overlap it */}
        <div className="relative z-20 mx-auto w-full max-w-[700px] pb-6 pt-3">
          <Console>
            {step === 'overview' && (
              <OverviewPane
                plans={plans}
                loading={plansQuery.isLoading}
                error={plansQuery.isError}
                onRetry={() => void plansQuery.refetch()}
                selectedId={planId}
                onSelect={setPlanId}
                onIgnite={goMethod}
                onRedeem={() => setStep('redeem')}
              />
            )}
            {step === 'method' && (
              <MethodPane
                options={options}
                selected={option}
                onSelect={setOption}
                canRecur={canRecur}
                recurring={recurring}
                onRecurring={setRecurring}
                amount={selectedPlan ? `${selectedPlan.price_rub} ₽` : ''}
                pending={checkoutMut.isPending}
                error={checkoutMut.isError}
                onContinue={() => checkoutMut.mutate()}
              />
            )}
            {step === 'pay' && checkout && option && (
              <PayPane
                checkout={checkout}
                option={option}
                phase={phase}
                onChangeMethod={goMethod}
              />
            )}
            {step === 'success' && (
              <SuccessPane onMusic={() => navigate('/home')} onManage={() => setStep('manage')} />
            )}
            {step === 'manage' && (
              <ManagePane onExtend={() => setStep('overview')} onRedeem={() => setStep('redeem')} />
            )}
            {step === 'redeem' && <RedeemPane onRedeemed={() => setStep('manage')} />}
          </Console>
        </div>
      </div>
    </div>
  );
});

export default StarPage;
