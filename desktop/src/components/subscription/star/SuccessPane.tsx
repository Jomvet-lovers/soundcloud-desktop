import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { PERKS } from './meta';
import { GhostBtn, PrimaryBtn } from './StarConsole';

/** Activated — what's now unlocked, plus go-to-music / manage. */
export const SuccessPane = memo(function SuccessPane({
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
