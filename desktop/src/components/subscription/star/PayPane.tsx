import { openUrl } from '@tauri-apps/plugin-opener';
import { memo, useCallback, useEffect, useRef } from 'react';
import { useTranslation } from 'react-i18next';
import { ExternalLink } from '../../../lib/icons';
import type { CheckoutResp } from '../../../lib/pay-client';
import { passSerial } from '../../../lib/star-format';
import { QrCode } from '../../auth/QrCode';
import type { PayPhase } from '../PayStatus';
import type { ActivationOption } from '../providers';
import { GhostBtn, LinkBtn, PrimaryBtn, Ttl } from './StarConsole';

/** Pay step — SBP QR with steps, or an external/crypto checkout opened in browser. */
export const PayPane = memo(function PayPane({
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
