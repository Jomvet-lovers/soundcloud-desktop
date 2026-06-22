import { useMutation } from '@tanstack/react-query';
import { memo, useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { payApi } from '../../../lib/pay-client';
import { requestPremiumRecheck } from '../../../lib/premium-cache';
import { CODE_RE, formatCode, normalizeBody, redeemErrorKey } from './redeem-code';
import { LinkBtn, PrimaryBtn, Ttl } from './StarConsole';

/** Redeem a STAR code (reseller receipts). */
export const RedeemPane = memo(function RedeemPane({ onRedeemed }: { onRedeemed: () => void }) {
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
