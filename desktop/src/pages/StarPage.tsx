import { useMutation, useQuery } from '@tanstack/react-query';
import { memo, useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { BoostyBlock } from '../components/subscription/BoostyBlock';
import { ExternalPay } from '../components/subscription/ExternalPay';
import { STAR_KEYFRAMES } from '../components/subscription/keyframes';
import { ManageSubscription } from '../components/subscription/ManageSubscription';
import { phaseOf } from '../components/subscription/PayStatus';
import { Perks } from '../components/subscription/Perks';
import { PlanSelect } from '../components/subscription/PlanSelect';
import { ProviderSelect } from '../components/subscription/ProviderSelect';
import {
  type ActivationOption,
  activationOptions,
  toCheckout,
} from '../components/subscription/providers';
import { RedeemCode } from '../components/subscription/RedeemCode';
import { SbpQrPay } from '../components/subscription/SbpQrPay';
import { StarAtmosphere } from '../components/subscription/StarAtmosphere';
import { StarPass } from '../components/subscription/StarPass';
import { GlassButton } from '../components/ui/GlassButton';
import { ArrowRight, ChevronLeft } from '../lib/icons';
import { type CheckoutResp, type Plan, payApi } from '../lib/pay-client';
import { usePerfMode } from '../lib/perf';
import { usePremium } from '../lib/premium-cache';
import { passDate, passSerial } from '../lib/star-format';
import { useOrderPoll } from '../lib/useOrderPoll';
import { useAuthStore } from '../stores/auth';

type Step = 'hero' | 'plan' | 'provider' | 'pay' | 'success' | 'redeem' | 'manage';

function planLabel(
  t: (k: string, o?: Record<string, unknown>) => string,
  plan: Plan | null,
): string {
  if (!plan) return '';
  const key = plan.months >= 12 ? 'year' : plan.months >= 3 ? 'quarter' : 'month';
  const cls = plan.months >= 12 ? 'S' : plan.months >= 3 ? 'B' : 'A';
  return `${t(`starpass.plan.${key}`)} · ${t('starpass.classLabel', { cls })}`;
}

const SectionHead = memo(function SectionHead({
  no,
  title,
  sub,
  onBack,
}: {
  no: string;
  title: string;
  sub?: string;
  onBack?: () => void;
}) {
  return (
    <div className="mb-7 flex flex-wrap items-baseline gap-4">
      {onBack && (
        <button
          type="button"
          onClick={onBack}
          className="grid size-8 cursor-pointer place-items-center rounded-lg border border-white/[0.06] text-white/55 transition-colors hover:text-white/90"
        >
          <ChevronLeft size={16} />
        </button>
      )}
      <span
        className="rounded-[7px] px-[9px] py-1 font-mono text-[11px] tracking-[0.2em] text-accent"
        style={{ border: '1px solid color-mix(in srgb, var(--color-accent) 40%, transparent)' }}
      >
        {no}
      </span>
      <h2
        className="text-[24px] font-medium tracking-[-0.01em]"
        style={{ fontFamily: 'var(--font-serif)' }}
      >
        {title}
      </h2>
      {sub && <p className="ml-auto max-w-[360px] text-right text-[14px] text-white/55">{sub}</p>}
    </div>
  );
});

export const StarPage = memo(function StarPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const perf = usePerfMode();
  const premium = usePremium();
  const username = useAuthStore((s) => s.user?.username);
  const handle = username ? `@${username}` : t('starpass.guestHandle');

  const [step, setStep] = useState<Step>(premium ? 'manage' : 'hero');
  const [planId, setPlanId] = useState<string | null>(null);
  const [option, setOption] = useState<ActivationOption | null>(null);
  const [recurring, setRecurring] = useState(false);
  const [checkout, setCheckout] = useState<CheckoutResp | null>(null);

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

  // Auto-pick the best plan once plans arrive.
  const bestId = plans.reduce<Plan | null>(
    (b, p) => (!b || p.savings_pct > b.savings_pct ? p : b),
    null,
  )?.id;
  if (!planId && bestId) setPlanId(bestId);

  const order = useOrderPoll(
    step === 'pay' || step === 'success' ? (checkout?.order_id ?? null) : null,
  );
  const phase = phaseOf(order.data?.status);
  // Drive to the success screen once the order is granted.
  if (step === 'pay' && phase === 'granted') setStep('success');

  const checkoutMut = useMutation({
    mutationFn: () => {
      if (!option || !planId) throw new Error('no selection');
      const allowRecurring = option.kind === 'tgstars' && selectedPlan?.months === 1 && recurring;
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

  const goProvider = useCallback(() => {
    reset();
    setStep('provider');
  }, [reset]);

  const tierLabel = planLabel(t, selectedPlan);
  const canRecur = option?.kind === 'tgstars' && selectedPlan?.months === 1;

  return (
    <>
      <style>{STAR_KEYFRAMES}</style>
      <div className="relative w-full min-h-screen">
        <StarAtmosphere />
        <div
          className="relative z-10 mx-auto w-full max-w-[1180px] px-4 pb-32 pt-10 md:px-8 md:pt-16"
          style={{ isolation: 'isolate' }}
        >
          {/* ── MANAGE ─────────────────────────────────────────── */}
          {step === 'manage' && (
            <Reveal idle={perf.idleAnim}>
              <SectionHead
                no="06"
                title={t('starpass.manage.title')}
                sub={t('starpass.manage.sub')}
              />
              <ManageSubscription
                handle={handle}
                onRedeem={() => setStep('redeem')}
                onExtend={() => setStep('plan')}
              />
            </Reveal>
          )}

          {/* ── HERO ───────────────────────────────────────────── */}
          {step === 'hero' && (
            <Reveal idle={perf.idleAnim}>
              <section className="grid grid-cols-1 items-center gap-12 lg:grid-cols-[1.05fr_1fr]">
                <div>
                  <div className="mb-[22px] flex items-center gap-3">
                    <span className="h-px w-[46px] bg-white/[0.12]" />
                    <span className="font-mono text-[11px] uppercase tracking-[0.24em] text-white/35">
                      {t('starpass.eyebrow')}
                    </span>
                  </div>
                  <h1
                    className="mb-[22px] text-[clamp(40px,5.4vw,64px)] font-medium leading-[0.98] tracking-[-0.02em]"
                    style={{ fontFamily: 'var(--font-serif)' }}
                  >
                    {t('starpass.heroTitle1')}
                    <br />
                    <em
                      className="not-italic"
                      style={{
                        fontStyle: 'italic',
                        background:
                          'linear-gradient(120deg, color-mix(in srgb, var(--color-accent) 60%, #fff), var(--color-accent), color-mix(in srgb, var(--color-accent) 82%, #fff))',
                        backgroundSize: '200%',
                        WebkitBackgroundClip: 'text',
                        backgroundClip: 'text',
                        color: 'transparent',
                        animation: perf.idleAnim ? 'star-foil-text 11s linear infinite' : undefined,
                      }}
                    >
                      {t('starpass.heroTitle2')}
                    </em>
                  </h1>
                  <p className="mb-[30px] max-w-[440px] text-[17px] leading-[1.55] text-white/55">
                    {t('starpass.heroLead')}
                  </p>
                  <div className="flex flex-wrap items-center gap-3">
                    <GlassButton variant="primary" onClick={() => setStep('plan')}>
                      {t('starpass.choosePass')}
                      <ArrowRight size={16} />
                    </GlassButton>
                    <GlassButton variant="ghost" onClick={() => setStep('redeem')}>
                      {t('starpass.haveCode')}
                    </GlassButton>
                  </div>
                  <BoostyBlock />
                </div>
                <StarPass
                  variant="hero"
                  handle={handle}
                  caption={t('starpass.caption.member')}
                  tier={t('starpass.tierPreview')}
                  fields={[
                    { label: t('starpass.fieldSerial'), value: passSerial(handle) },
                    {
                      label: t('starpass.fieldIssued'),
                      value: passDate(Math.floor(Date.now() / 1000)),
                    },
                    {
                      label: t('starpass.fieldValidUntil'),
                      value: t('starpass.afterPurchase'),
                      big: true,
                    },
                  ]}
                />
              </section>
              <div className="mt-[46px]">
                <Perks />
              </div>
            </Reveal>
          )}

          {/* ── PLAN SELECT ────────────────────────────────────── */}
          {step === 'plan' && (
            <Reveal idle={perf.idleAnim}>
              <SectionHead
                no="02"
                title={t('starpass.planTitle')}
                sub={t('starpass.planSub')}
                onBack={() => setStep(premium ? 'manage' : 'hero')}
              />
              {plansQuery.isError ? (
                <div className="rounded-2xl border border-red-400/20 bg-red-500/[0.06] px-5 py-4">
                  <p className="text-[14px] text-red-200/90">{t('starpass.loadError')}</p>
                  <p className="mt-1 font-mono text-[12px] text-white/40">
                    {String((plansQuery.error as Error)?.message ?? '').slice(0, 180)}
                  </p>
                  <GlassButton
                    variant="ghost"
                    className="mt-3"
                    onClick={() => void plansQuery.refetch()}
                  >
                    {t('starpass.retry')}
                  </GlassButton>
                </div>
              ) : plans.length === 0 ? (
                <p className="text-[14px] text-white/55">{t('starpass.loading')}</p>
              ) : (
                <>
                  <PlanSelect plans={plans} selectedId={planId} onSelect={setPlanId} />
                  <div className="mt-6 flex flex-wrap items-center gap-3">
                    <GlassButton
                      variant="primary"
                      disabled={!selectedPlan}
                      onClick={() => setStep('provider')}
                    >
                      {t('starpass.toActivation')}
                      {selectedPlan && ` · ${selectedPlan.price_rub} ₽`}
                    </GlassButton>
                    {selectedPlan && (
                      <span className="rounded-full border border-white/[0.06] bg-white/[0.03] px-[11px] py-[6px] font-mono text-[11px] text-white/55">
                        {tierLabel}
                      </span>
                    )}
                  </div>
                </>
              )}
            </Reveal>
          )}

          {/* ── PROVIDER SELECT ────────────────────────────────── */}
          {step === 'provider' && (
            <Reveal idle={perf.idleAnim}>
              <SectionHead
                no="03"
                title={t('starpass.providerTitle')}
                sub={t('starpass.providerSub')}
                onBack={() => setStep('plan')}
              />
              <ProviderSelect
                options={options}
                selectedKind={option?.kind ?? null}
                onSelect={setOption}
              />
              {canRecur && (
                <label className="mt-5 flex w-fit cursor-pointer items-center gap-3 rounded-2xl border border-white/[0.06] bg-white/[0.03] px-4 py-3 text-[13px] text-white/75">
                  <input
                    type="checkbox"
                    checked={recurring}
                    onChange={(e) => setRecurring(e.target.checked)}
                    className="size-4 accent-[var(--color-accent)]"
                  />
                  {t('starpass.recurring')}
                </label>
              )}
              <div className="mt-6 flex flex-wrap items-center gap-3">
                <GlassButton
                  variant="primary"
                  disabled={!option || checkoutMut.isPending}
                  onClick={() => checkoutMut.mutate()}
                >
                  {checkoutMut.isPending
                    ? t('starpass.creating')
                    : option
                      ? t('starpass.continueWith', {
                          method: t(`starpass.method.${option.i18n}.title`),
                        })
                      : t('starpass.continue')}
                </GlassButton>
                {selectedPlan && (
                  <span className="rounded-full border border-white/[0.06] bg-white/[0.03] px-[11px] py-[6px] font-mono text-[11px] text-white/55">
                    {t('starpass.toPay')}: {selectedPlan.price_rub} ₽
                  </span>
                )}
              </div>
              {checkoutMut.isError && (
                <p className="mt-3 text-[13px] text-red-300/95">{t('starpass.checkoutError')}</p>
              )}
            </Reveal>
          )}

          {/* ── PAY ────────────────────────────────────────────── */}
          {step === 'pay' && checkout && option && (
            <Reveal idle={perf.idleAnim}>
              <SectionHead
                no="04"
                title={t('starpass.payTitle')}
                sub={t('starpass.paySub')}
                onBack={goProvider}
              />
              {option.method === 'sbp' && checkout.sbp_qr ? (
                <SbpQrPay
                  checkout={checkout}
                  handle={handle}
                  tier={tierLabel}
                  phase={phase}
                  onChangeMethod={goProvider}
                />
              ) : (
                <ExternalPay
                  checkout={checkout}
                  handle={handle}
                  tier={tierLabel}
                  methodLabel={t(`starpass.method.${option.i18n}.title`)}
                  phase={phase}
                  onChangeMethod={goProvider}
                />
              )}
              {phase === 'failed' && (
                <div className="mt-6">
                  <GlassButton variant="primary" onClick={goProvider}>
                    {t('starpass.retry')}
                  </GlassButton>
                </div>
              )}
            </Reveal>
          )}

          {/* ── SUCCESS ────────────────────────────────────────── */}
          {step === 'success' && (
            <Reveal idle={perf.idleAnim}>
              <SectionHead
                no="05"
                title={t('starpass.successTitle')}
                sub={t('starpass.successSub')}
              />
              <SuccessScreen
                handle={handle}
                tier={tierLabel}
                endsAt={order.data?.premium_until ?? checkout?.expires_at ?? 0}
                serialSeed={checkout?.order_id ?? handle}
                methodLabel={option ? t(`starpass.method.${option.i18n}.title`) : ''}
                onMusic={() => navigate('/home')}
                onManage={() => setStep('manage')}
              />
            </Reveal>
          )}

          {/* ── REDEEM ─────────────────────────────────────────── */}
          {step === 'redeem' && (
            <Reveal idle={perf.idleAnim}>
              <SectionHead
                no="07"
                title={t('starpass.redeemTitle')}
                sub={t('starpass.redeemSub')}
                onBack={() => setStep(premium ? 'manage' : 'hero')}
              />
              <RedeemCode onRedeemed={() => setStep('manage')} />
            </Reveal>
          )}
        </div>
      </div>
    </>
  );
});

/* ── success screen ─────────────────────────────────────────────── */
const SuccessScreen = memo(function SuccessScreen({
  handle,
  tier,
  endsAt,
  serialSeed,
  methodLabel,
  onMusic,
  onManage,
}: {
  handle: string;
  tier: string;
  endsAt: number;
  serialSeed: string;
  methodLabel: string;
  onMusic: () => void;
  onManage: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="grid grid-cols-1 items-center gap-9 lg:grid-cols-2">
      <div>
        <h2
          className="mb-[14px] text-[38px] font-medium leading-[1.02] tracking-[-0.01em]"
          style={{ fontFamily: 'var(--font-serif)' }}
        >
          {t('starpass.welcome', { handle })}
        </h2>
        <p className="max-w-[380px] text-[15px] text-white/55">
          {t('starpass.welcomeBody', { date: passDate(endsAt) })}
        </p>
        <div className="mt-[18px] flex flex-wrap gap-[14px]">
          <Chip>
            <b className="text-accent">★</b> {tier}
          </Chip>
          <Chip>
            {t('starpass.until')} <b className="text-accent">{passDate(endsAt)}</b>
          </Chip>
          {methodLabel && (
            <Chip>
              {t('starpass.fieldMethod')} <b className="text-accent">{methodLabel}</b>
            </Chip>
          )}
        </div>
        <div className="mt-6 flex flex-wrap gap-3">
          <GlassButton variant="primary" onClick={onMusic}>
            {t('starpass.backToMusic')}
          </GlassButton>
          <GlassButton variant="ghost" onClick={onManage}>
            {t('starpass.managePass')}
          </GlassButton>
        </div>
      </div>
      <StarPass
        variant="activated"
        stamped
        handle={handle}
        caption={t('starpass.caption.active')}
        tier={tier}
        fields={[
          { label: t('starpass.fieldSerial'), value: passSerial(serialSeed) },
          { label: t('starpass.fieldActivated'), value: passDate(Math.floor(Date.now() / 1000)) },
          { label: t('starpass.fieldValidUntil'), value: passDate(endsAt), big: true },
        ]}
      />
    </div>
  );
});

function Chip({ children }: { children: React.ReactNode }) {
  return (
    <span className="rounded-full border border-white/[0.06] bg-white/[0.03] px-[11px] py-[6px] font-mono text-[11px] text-white/55">
      {children}
    </span>
  );
}

/* ── load reveal (gated on perf.idleAnim / reduced-motion) ──────── */
function Reveal({ idle, children }: { idle: boolean; children: React.ReactNode }) {
  return (
    <div style={{ animation: idle ? 'star-reveal 0.5s var(--ease-apple) both' : undefined }}>
      {children}
    </div>
  );
}

export default StarPage;
