import { useMutation, useQuery } from '@tanstack/react-query';
import { memo, useCallback } from 'react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { type Entitlement, payApi } from '../../lib/pay-client';
import { requestPremiumRecheck } from '../../lib/premium-cache';
import { queryClient } from '../../lib/query-client';
import { daysUntil, passDate, passSerial } from '../../lib/star-format';
import { GlassButton } from '../ui/GlassButton';
import { StarPass } from './StarPass';

const SUB_KEY = ['pay', 'subscription'] as const;

/** Pick the entitlement that defines the active window (latest end). */
function primaryEntitlement(ents: Entitlement[]): Entitlement | null {
  return ents.reduce<Entitlement | null>(
    (best, e) => (!best || e.ends_at > best.ends_at ? e : best),
    null,
  );
}

const RowCard = memo(function RowCard({
  label,
  sub,
  children,
}: {
  label: string;
  sub?: string;
  children: React.ReactNode;
}) {
  return (
    <div
      className="flex items-center justify-between gap-[14px] rounded-[14px] border border-white/[0.06] px-[18px] py-4"
      style={{ background: 'rgba(255,255,255,0.03)' }}
    >
      <div>
        <div className="text-[13px] text-white/55">{label}</div>
        {sub && <div className="mt-[2px] text-[12px] text-white/35">{sub}</div>}
      </div>
      {children}
    </div>
  );
});

interface ManageSubscriptionProps {
  handle: string;
  onRedeem: () => void;
  onExtend: () => void;
}

export const ManageSubscription = memo(function ManageSubscription({
  handle,
  onRedeem,
  onExtend,
}: ManageSubscriptionProps) {
  const { t } = useTranslation();
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

  const onToggle = useCallback(() => {
    // Cancel only turns auto-renew OFF; there is no re-enable via the API.
    if (autoRenew && !cancel.isPending) cancel.mutate();
  }, [autoRenew, cancel]);

  // Serial is a stable display id from the handle — present even before pay detail loads.
  const serial = passSerial(handle);

  return (
    <div className="grid grid-cols-1 items-start gap-7 lg:grid-cols-[1fr_360px]">
      <StarPass
        variant="manage"
        handle={handle}
        caption={t('starpass.caption.active')}
        tier={t('starpass.tierActive')}
        fields={[
          { label: t('starpass.fieldSerial'), value: serial },
          {
            label: t('starpass.fieldRemaining'),
            value: t('starpass.daysLeft', { count: daysUntil(endsAt) }),
          },
          { label: t('starpass.fieldValidUntil'), value: passDate(endsAt), big: true },
        ]}
      />

      <div className="flex flex-col gap-[14px]">
        <RowCard
          label={t('starpass.manage.autoRenew')}
          sub={
            autoRenew
              ? t('starpass.manage.autoRenewOn', { date: passDate(endsAt) })
              : t('starpass.manage.autoRenewOff')
          }
        >
          <button
            type="button"
            role="switch"
            aria-checked={autoRenew}
            aria-label={t('starpass.manage.autoRenew')}
            disabled={!autoRenew || cancel.isPending || isLoading || !source}
            onClick={onToggle}
            className="relative h-7 w-12 shrink-0 cursor-pointer rounded-full border transition-all duration-300 ease-[var(--ease-apple)] disabled:cursor-default"
            style={{
              borderColor: 'rgba(255,255,255,0.12)',
              background: autoRenew
                ? 'color-mix(in srgb, var(--color-accent) 22%, transparent)'
                : 'rgba(255,255,255,0.04)',
            }}
          >
            <span
              className="absolute top-[2px] size-[22px] rounded-full transition-all duration-300 ease-[var(--ease-apple)]"
              style={{
                left: autoRenew ? 22 : 2,
                background: autoRenew ? 'var(--color-accent)' : 'rgba(255,255,255,0.4)',
                boxShadow: '0 2px 6px rgba(0,0,0,0.4)',
              }}
            />
          </button>
        </RowCard>

        <RowCard label={t('starpass.manage.source')} sub={t('starpass.manage.sourceSub')}>
          <span className="font-mono text-[14px] text-white/90">{source || '—'}</span>
        </RowCard>

        <RowCard label={t('starpass.fieldValidUntil')} sub={t('starpass.manage.afterExpiry')}>
          <span className="font-mono text-[14px] tabular-nums text-white/90">
            {passDate(endsAt)}
          </span>
        </RowCard>

        <div className="mt-1 flex flex-wrap gap-3">
          <GlassButton variant="ghost" onClick={onExtend}>
            {t('starpass.manage.extend')}
          </GlassButton>
          <GlassButton variant="ghost" onClick={onRedeem}>
            {t('starpass.manage.redeem')}
          </GlassButton>
        </div>
      </div>
    </div>
  );
});
