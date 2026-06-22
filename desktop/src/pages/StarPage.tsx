import { useMutation, useQuery } from '@tanstack/react-query';
import { openUrl } from '@tauri-apps/plugin-opener';
import { memo, useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import { QrCode } from '../components/auth/QrCode';
import { DiscordCard } from '../components/subscription/DiscordCard';
import { CORE_CENTER_Y, LivingCore } from '../components/subscription/LivingCore';
import { type PayPhase, phaseOf } from '../components/subscription/PayStatus';
import { ProviderGlyph } from '../components/subscription/ProviderGlyph';
import {
  type ActivationOption,
  activationOptions,
  toCheckout,
} from '../components/subscription/providers';
import { StarAtmosphere } from '../components/subscription/StarAtmosphere';
import {
  ArrowRight,
  AudioLines,
  Check,
  ChevronLeft,
  Database,
  ExternalLink,
  Globe,
  Heart,
  Loader2,
  Music,
  Sparkles,
} from '../lib/icons';
import {
  type CheckoutResp,
  type Entitlement,
  PayError,
  type Plan,
  payApi,
} from '../lib/pay-client';
import { requestPremiumRecheck, usePremium } from '../lib/premium-cache';
import { queryClient } from '../lib/query-client';
import { daysUntil, passDate, passSerial } from '../lib/star-format';
import { useOrderPoll } from '../lib/useOrderPoll';
import { useAuthStore } from '../stores/auth';

type Step = 'overview' | 'method' | 'pay' | 'success' | 'redeem' | 'manage';

const PERKS = [
  { key: 'goPlus', Icon: Music },
  { key: 'server', Icon: Database },
  { key: 'hq', Icon: AudioLines },
  { key: 'whitelist', Icon: Globe },
  { key: 'soundwave', Icon: Sparkles },
  { key: 'support', Icon: Heart },
] as const;

function monthsKey(months: number): 'year' | 'quarter' | 'month' {
  return months >= 12 ? 'year' : months >= 3 ? 'quarter' : 'month';
}

/* ── redeem code helpers (kept thin, pure) ─────────────────────── */
const CODE_RE = /^STAR(-[0-9A-Z]{4}){4}$/;
function normalizeBody(raw: string): string {
  let s = raw.toUpperCase().replace(/[^0-9A-Z]/g, '');
  if (s.startsWith('STAR')) s = s.slice(4);
  return s.slice(0, 16);
}
function formatCode(body: string): string {
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

function primaryEntitlement(ents: Entitlement[]): Entitlement | null {
  return ents.reduce<Entitlement | null>((b, e) => (!b || e.ends_at > b.ends_at ? e : b), null);
}

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
  const charge = selectedPlan
    ? selectedPlan.months >= 12
      ? 1
      : selectedPlan.months >= 3
        ? 0.6
        : 0.28
    : 0.5;

  return (
    <div className="relative w-full min-h-full">
      <StarAtmosphere />
      <div
        className="relative z-10 mx-auto flex w-full max-w-[1060px] flex-col px-4 md:px-8"
        style={{ isolation: 'isolate' }}
      >
        <div className="relative min-h-[640px] h-[calc(100vh-190px)]">
          {/* living core backdrop */}
          <LivingCore
            charge={charge}
            waiting={step === 'pay' && phase === 'waiting'}
            lit={lit}
            igniteKey={igniteKey}
          />

          {/* top chrome */}
          <div className="pointer-events-none absolute inset-x-0 top-5 z-20 flex items-center justify-between px-1">
            <div className="flex items-center gap-2.5 font-mono text-[12px] uppercase tracking-[0.34em] text-white/60">
              <span className="text-accent">✦</span> STAR
            </div>
            {(step === 'method' || step === 'pay' || step === 'redeem') && (
              <button
                type="button"
                onClick={() => setStep(step === 'pay' ? 'method' : premium ? 'manage' : 'overview')}
                className="pointer-events-auto flex cursor-pointer items-center gap-1.5 font-mono text-[11px] tracking-[0.14em] text-white/45 transition-colors hover:text-white/90"
              >
                <ChevronLeft size={14} /> {t('starpass.back')}
              </button>
            )}
          </div>

          {/* aperture centre readout */}
          <div
            className="pointer-events-none absolute left-1/2 z-20 w-[300px] -translate-x-1/2 -translate-y-1/2 text-center"
            style={{ top: `${CORE_CENTER_Y * 100}%` }}
          >
            <CenterReadout
              step={step}
              phase={phase}
              handle={handle}
              plan={selectedPlan}
              endsAt={order.data?.premium_until ?? checkout?.expires_at ?? 0}
              serialSeed={checkout?.order_id ?? handle}
            />
          </div>

          {/* console */}
          <div className="absolute inset-x-2 bottom-6 z-20 mx-auto w-full max-w-[700px] sm:inset-x-auto sm:left-1/2 sm:-translate-x-1/2">
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
                <ManagePane onExtend={goMethod} onRedeem={() => setStep('redeem')} />
              )}
              {step === 'redeem' && <RedeemPane onRedeemed={() => setStep('manage')} />}
            </Console>
          </div>
        </div>
      </div>
    </div>
  );
});

/* ── centre well content ───────────────────────────────────────── */
const CenterReadout = memo(function CenterReadout({
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

/* ── console shell ─────────────────────────────────────────────── */
const Console = memo(function Console({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="relative rounded-[22px] border border-white/[0.10] p-5 md:p-[22px]"
      style={{
        background:
          'linear-gradient(180deg, rgba(255,255,255,0.05), rgba(255,255,255,0.012) 60%), rgba(12,11,16,0.72)',
        backdropFilter: 'blur(26px) saturate(1.5)',
        WebkitBackdropFilter: 'blur(26px) saturate(1.5)',
        boxShadow: '0 30px 90px -30px rgba(0,0,0,0.9), inset 0 1px 0 rgba(255,255,255,0.08)',
      }}
    >
      <span
        aria-hidden
        className="absolute inset-x-6 top-0 h-px"
        style={{
          background: 'linear-gradient(90deg, transparent, rgba(255,255,255,0.4), transparent)',
        }}
      />
      {children}
    </div>
  );
});

const PrimaryBtn = memo(function PrimaryBtn({
  children,
  onClick,
  disabled,
}: {
  children: React.ReactNode;
  onClick: () => void;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled}
      className="inline-flex cursor-pointer items-center gap-2 rounded-[13px] px-6 py-3.5 text-[13.5px] font-semibold text-accent-contrast transition-transform duration-200 ease-[var(--ease-apple)] hover:-translate-y-px disabled:cursor-default disabled:opacity-50"
      style={{
        background: 'linear-gradient(180deg, var(--color-accent-hover), var(--color-accent))',
        boxShadow: '0 0 36px -8px var(--color-accent-glow), inset 0 1px 0 rgba(255,255,255,0.4)',
      }}
    >
      {children}
    </button>
  );
});

const GhostBtn = memo(function GhostBtn({
  children,
  onClick,
}: {
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="inline-flex cursor-pointer items-center gap-2 rounded-[13px] border border-white/[0.10] bg-white/[0.05] px-5 py-3.5 text-[13.5px] font-semibold text-white/80 transition-colors hover:bg-white/[0.09] hover:text-white"
    >
      {children}
    </button>
  );
});

const LinkBtn = memo(function LinkBtn({
  children,
  onClick,
}: {
  children: React.ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="cursor-pointer bg-transparent font-mono text-[11px] uppercase tracking-[0.12em] text-white/40 transition-colors hover:text-white/80"
    >
      {children}
    </button>
  );
});

const Ttl = memo(function Ttl({ children }: { children: React.ReactNode }) {
  return (
    <div className="mb-3.5 font-mono text-[10.5px] uppercase tracking-[0.2em] text-white/40">
      {children}
    </div>
  );
});

/* ── overview pane ─────────────────────────────────────────────── */
const OverviewPane = memo(function OverviewPane({
  plans,
  loading,
  error,
  onRetry,
  selectedId,
  onSelect,
  onIgnite,
  onRedeem,
}: {
  plans: Plan[];
  loading: boolean;
  error: boolean;
  onRetry: () => void;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onIgnite: () => void;
  onRedeem: () => void;
}) {
  const { t } = useTranslation();

  if (error) {
    return (
      <div>
        <p className="text-[14px] text-red-200/90">{t('starpass.loadError')}</p>
        <div className="mt-3">
          <GhostBtn onClick={onRetry}>{t('starpass.retry')}</GhostBtn>
        </div>
      </div>
    );
  }

  return (
    <div>
      {/* duration segmented */}
      <div className="flex gap-1.5 rounded-[14px] border border-white/[0.10] bg-white/[0.04] p-1.5">
        {(loading ? [] : plans).map((p) => {
          const on = p.id === selectedId;
          return (
            <button
              key={p.id}
              type="button"
              onClick={() => onSelect(p.id)}
              className="flex-1 cursor-pointer rounded-[10px] px-2 py-2.5 text-center font-mono text-[12px] transition-all duration-200"
              style={{
                color: on ? '#fff' : 'rgba(255,255,255,0.5)',
                background: on
                  ? 'linear-gradient(180deg, color-mix(in srgb, var(--color-accent) 28%, transparent), transparent), rgba(255,255,255,0.06)'
                  : undefined,
                boxShadow: on
                  ? '0 0 18px -4px var(--color-accent-glow), inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 45%, transparent)'
                  : undefined,
              }}
            >
              {t(`starpass.plan.${monthsKey(p.months)}`)}
              <span className="mt-1 block text-[9.5px] text-white/40">
                {p.savings_pct > 0 ? `−${p.savings_pct}%` : `${p.price_rub} ₽`}
              </span>
            </button>
          );
        })}
        {loading && (
          <div className="flex-1 py-2.5 text-center font-mono text-[12px] text-white/40">
            {t('starpass.loading')}
          </div>
        )}
      </div>

      {/* perks */}
      <div className="mt-4 grid grid-cols-2 gap-2 sm:grid-cols-3">
        {PERKS.map(({ key, Icon }) => (
          <div
            key={key}
            className="flex items-center gap-2.5 rounded-[11px] border border-white/[0.08] bg-white/[0.025] px-3 py-2.5"
          >
            <Icon size={15} className="shrink-0 text-accent" />
            <span className="truncate text-[12px] font-medium text-white/85">
              {t(`starpass.perk.${key}.title`)}
            </span>
          </div>
        ))}
      </div>

      <div className="mt-[18px] flex flex-wrap items-center gap-3">
        <PrimaryBtn onClick={onIgnite} disabled={!selectedId}>
          {t('starpass.ignite')}
          <ArrowRight size={16} />
        </PrimaryBtn>
        <LinkBtn onClick={onRedeem}>{t('starpass.haveCode')}</LinkBtn>
      </div>
    </div>
  );
});

/* ── method pane ───────────────────────────────────────────────── */
const MethodPane = memo(function MethodPane({
  options,
  selected,
  onSelect,
  canRecur,
  recurring,
  onRecurring,
  amount,
  pending,
  error,
  onContinue,
}: {
  options: ActivationOption[];
  selected: ActivationOption | null;
  onSelect: (o: ActivationOption) => void;
  canRecur: boolean;
  recurring: boolean;
  onRecurring: (v: boolean) => void;
  amount: string;
  pending: boolean;
  error: boolean;
  onContinue: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div>
      <Ttl>{t('starpass.providerSub')}</Ttl>
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-5">
        {options.map((opt) => {
          const on = opt.kind === selected?.kind;
          return (
            <button
              key={opt.kind}
              type="button"
              onClick={() => onSelect(opt)}
              className="cursor-pointer rounded-[13px] border p-3 text-left transition-all duration-200 ease-[var(--ease-apple)] hover:-translate-y-0.5"
              style={{
                borderColor: on
                  ? 'color-mix(in srgb, var(--color-accent) 60%, transparent)'
                  : 'rgba(255,255,255,0.10)',
                background: 'rgba(255,255,255,0.025)',
                boxShadow: on
                  ? '0 0 24px -8px var(--color-accent-glow), inset 0 0 0 1px color-mix(in srgb, var(--color-accent) 35%, transparent)'
                  : undefined,
              }}
            >
              <span className="mb-2.5 grid size-[30px] place-items-center rounded-[9px] border border-white/[0.08] bg-white/[0.04]">
                <ProviderGlyph kind={opt.kind} />
              </span>
              <div className="text-[12.5px] font-semibold text-white/90">
                {t(`starpass.method.${opt.i18n}.title`)}
              </div>
              <div className="mt-0.5 font-mono text-[9px] tracking-[0.06em] text-white/40">
                {opt.tag}
              </div>
            </button>
          );
        })}
      </div>

      {/* recurring — only for methods that support it */}
      {canRecur && (
        <label className="mt-3.5 flex w-fit cursor-pointer items-center gap-3 rounded-[12px] border border-white/[0.10] bg-white/[0.03] px-3.5 py-2.5">
          <input
            type="checkbox"
            checked={recurring}
            onChange={(e) => onRecurring(e.target.checked)}
            className="size-4 accent-[var(--color-accent)]"
          />
          <span>
            <span className="block text-[12.5px] font-medium text-white/85">
              {t('starpass.recurring')}
            </span>
            <span className="block text-[11px] text-white/40">{t('starpass.recurringSub')}</span>
          </span>
        </label>
      )}

      <div className="mt-[18px] flex flex-wrap items-center gap-3">
        <PrimaryBtn onClick={onContinue} disabled={!selected || pending}>
          {pending ? t('starpass.creating') : `${t('starpass.continue')} · ${amount}`}
        </PrimaryBtn>
        {error && (
          <span className="text-[12.5px] text-red-300/95">{t('starpass.checkoutError')}</span>
        )}
      </div>
    </div>
  );
});

/* ── pay pane (SBP QR or external) ─────────────────────────────── */
const PayPane = memo(function PayPane({
  checkout,
  option,
  phase,
  onChangeMethod,
}: {
  checkout: CheckoutResp;
  option: ActivationOption;
  phase: PayPhase;
  onChangeMethod: () => void;
}) {
  const { t } = useTranslation();
  const serial = passSerial(checkout.order_id);
  const targets = checkout.pay_targets ?? [];
  const isSbp = option.method === 'sbp' && !!checkout.sbp_qr;

  const openedRef = useRef(false);
  const open = useCallback(() => {
    if (checkout.pay_url) void openUrl(checkout.pay_url).catch(() => {});
  }, [checkout.pay_url]);
  useEffect(() => {
    if (!isSbp && !openedRef.current && checkout.pay_url) {
      openedRef.current = true;
      open();
    }
  }, [isSbp, checkout.pay_url, open]);

  return (
    <div className="flex flex-col gap-4 sm:flex-row sm:items-center">
      {isSbp ? (
        <div className="shrink-0 self-center rounded-[13px] bg-white p-2.5">
          <QrCode payload={checkout.sbp_qr as string} size={132} />
        </div>
      ) : null}

      <div className="min-w-0 flex-1">
        <Ttl>
          {t(`starpass.method.${option.i18n}.title`)} · {option.tag} · {serial}
        </Ttl>
        {isSbp ? (
          <ol className="flex list-none flex-col gap-2 p-0">
            {[1, 2, 3].map((n) => (
              <li key={n} className="flex gap-2.5 text-[12.5px] text-white/60">
                <span className="min-w-[16px] font-mono text-[11px] text-accent">
                  {String(n).padStart(2, '0')}
                </span>
                {t(`starpass.sbpStep.${n}`)}
              </li>
            ))}
          </ol>
        ) : (
          <p className="text-[12.5px] leading-relaxed text-white/60">
            {t('starpass.externalHint')}
          </p>
        )}

        <div className="mt-3.5 flex flex-wrap items-center gap-2.5">
          {!isSbp &&
            (targets.length > 1 ? (
              targets.map((tg) => (
                <GhostBtn key={tg.kind} onClick={() => void openUrl(tg.url).catch(() => {})}>
                  {t(`starpass.openIn.${tg.kind}`)}
                  <ExternalLink size={13} />
                </GhostBtn>
              ))
            ) : (
              <PrimaryBtn onClick={open}>
                {t('starpass.openCheckout')}
                <ExternalLink size={14} />
              </PrimaryBtn>
            ))}
          {phase === 'failed' && (
            <PrimaryBtn onClick={onChangeMethod}>{t('starpass.retry')}</PrimaryBtn>
          )}
          <LinkBtn onClick={onChangeMethod}>{t('starpass.changeMethod')}</LinkBtn>
        </div>
      </div>
    </div>
  );
});

/* ── success pane ──────────────────────────────────────────────── */
const SuccessPane = memo(function SuccessPane({
  onMusic,
  onManage,
}: {
  onMusic: () => void;
  onManage: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div>
      <div className="flex flex-wrap gap-2">
        {PERKS.map(({ key }) => (
          <span
            key={key}
            className="rounded-full border border-white/[0.08] bg-white/[0.03] px-3 py-1.5 font-mono text-[10.5px] tracking-[0.08em] text-white/65"
          >
            {t(`starpass.perk.${key}.title`)}
          </span>
        ))}
      </div>
      <div className="mt-4 flex flex-wrap gap-3">
        <PrimaryBtn onClick={onMusic}>{t('starpass.backToMusic')}</PrimaryBtn>
        <GhostBtn onClick={onManage}>{t('starpass.managePass')}</GhostBtn>
      </div>
    </div>
  );
});

/* ── manage pane ───────────────────────────────────────────────── */
const ManagePane = memo(function ManagePane({
  onExtend,
  onRedeem,
}: {
  onExtend: () => void;
  onRedeem: () => void;
}) {
  const { t } = useTranslation();
  const SUB_KEY = ['pay', 'subscription'] as const;
  const { data, isLoading } = useQuery({
    queryKey: SUB_KEY,
    queryFn: payApi.subscription,
    staleTime: 30_000,
  });
  const ent = data ? primaryEntitlement(data.entitlements) : null;
  const source = ent?.source ?? '';
  const endsAt = ent?.ends_at ?? data?.premium_until ?? 0;
  const autoRenew = !!ent?.auto_renew && !ent?.canceled;

  const cancel = useMutation({
    mutationFn: () => payApi.cancel(source),
    onSuccess: () => {
      toast.success(t('starpass.manage.canceled'));
      void queryClient.invalidateQueries({ queryKey: SUB_KEY });
      requestPremiumRecheck();
    },
    onError: () => toast.error(t('starpass.manage.cancelError')),
  });

  return (
    <div className="flex flex-col gap-2.5">
      <DiscordCard discord={data?.discord ?? null} />

      <div className="flex items-center justify-between gap-3 rounded-[14px] border border-white/[0.08] bg-white/[0.03] px-4 py-3">
        <div>
          <div className="text-[13px] font-medium text-white/85">
            {t('starpass.manage.autoRenew')}
          </div>
          <div className="mt-0.5 text-[11.5px] text-white/40">
            {autoRenew
              ? t('starpass.manage.autoRenewOn', { date: passDate(endsAt) })
              : t('starpass.manage.autoRenewOff')}
          </div>
        </div>
        <button
          type="button"
          role="switch"
          aria-checked={autoRenew}
          disabled={!autoRenew || cancel.isPending || isLoading || !source}
          onClick={() => autoRenew && !cancel.isPending && cancel.mutate()}
          className="relative h-6 w-11 shrink-0 cursor-pointer rounded-full border border-white/[0.12] transition-all duration-300 disabled:cursor-default"
          style={{
            background: autoRenew
              ? 'color-mix(in srgb, var(--color-accent) 22%, transparent)'
              : 'rgba(255,255,255,0.04)',
          }}
        >
          <span
            className="absolute top-0.5 size-5 rounded-full transition-all duration-300"
            style={{
              left: autoRenew ? 22 : 2,
              background: autoRenew ? 'var(--color-accent)' : 'rgba(255,255,255,0.4)',
            }}
          />
        </button>
      </div>

      <div className="flex items-center justify-between gap-3 rounded-[14px] border border-white/[0.08] bg-white/[0.03] px-4 py-3">
        <div>
          <div className="text-[13px] font-medium text-white/85">{t('starpass.manage.source')}</div>
          <div className="mt-0.5 text-[11.5px] text-white/40">{t('starpass.manage.sourceSub')}</div>
        </div>
        <span className="font-mono text-[13px] text-white">{source || '—'}</span>
      </div>

      <div className="mt-1 flex flex-wrap gap-3">
        <GhostBtn onClick={onExtend}>{t('starpass.manage.extend')}</GhostBtn>
        <GhostBtn onClick={onRedeem}>{t('starpass.manage.redeem')}</GhostBtn>
      </div>
    </div>
  );
});

/* ── redeem pane ───────────────────────────────────────────────── */
const RedeemPane = memo(function RedeemPane({ onRedeemed }: { onRedeemed: () => void }) {
  const { t } = useTranslation();
  const [body, setBody] = useState('');
  const code = useMemo(() => formatCode(body), [body]);
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
      /* clipboard denied */
    }
  }, [mutation]);

  return (
    <div>
      <Ttl>{t('starpass.redeem.lead')}</Ttl>
      <input
        type="text"
        value={code}
        onChange={onChange}
        spellCheck={false}
        autoCapitalize="characters"
        placeholder="STAR-XXXX-XXXX-XXXX-XXXX"
        aria-label={t('starpass.redeem.label')}
        className="selectable w-full rounded-[13px] border bg-black/30 px-4 py-3.5 text-center font-mono text-[19px] uppercase tracking-[0.05em] text-white outline-none transition-colors placeholder:text-white/25 focus:border-accent"
        style={{ borderColor: valid ? 'var(--color-accent)' : 'rgba(255,255,255,0.10)' }}
      />
      {mutation.isError && (
        <div className="mt-2.5 text-[12.5px] text-red-300/95">
          {t(redeemErrorKey(mutation.error))}
        </div>
      )}
      <div className="mt-[18px] flex flex-wrap items-center gap-3">
        <PrimaryBtn onClick={() => mutation.mutate()} disabled={!valid || mutation.isPending}>
          {mutation.isPending ? t('starpass.redeem.activating') : t('starpass.redeem.activate')}
        </PrimaryBtn>
        <LinkBtn onClick={onPaste}>{t('starpass.redeem.paste')}</LinkBtn>
      </div>
    </div>
  );
});

export default StarPage;
