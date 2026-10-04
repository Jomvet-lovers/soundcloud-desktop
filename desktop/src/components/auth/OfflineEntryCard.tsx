import { useTranslation } from 'react-i18next';
import { ChevronRight, Download, Globe } from '../../lib/icons';

/** Secondary entry — browse the offline (cached) library without signing in. */
export function OfflineEntryCard({ onClick }: { onClick: () => void }) {
  const { t } = useTranslation();
  return (
    <button
      type="button"
      onClick={onClick}
      className="group relative w-full rounded-[22px] border border-white/[0.10] bg-[#141417] px-4 py-3.5 text-left transition-colors hover:border-white/[0.18] cursor-pointer"
    >
      <span className="relative flex items-center gap-3">
        <span className="relative flex size-11 shrink-0 items-center justify-center rounded-[16px] border border-white/[0.12] bg-white/[0.05]">
          <Globe size={18} className="text-sky-100/95" strokeWidth={1.7} />
          <span className="absolute -bottom-1 -right-1 flex size-[18px] items-center justify-center rounded-full border border-white/[0.18] bg-emerald-400/90">
            <Download size={10} strokeWidth={3} className="text-emerald-950" />
          </span>
        </span>

        <span className="min-w-0 flex-1">
          <span className="block text-[13.5px] font-semibold tracking-tight text-white/92">
            {t('auth.continueOffline')}
          </span>
          <span className="mt-0.5 block text-[11.5px] leading-snug text-white/45">
            {t('auth.continueOfflineDesc')}
          </span>
        </span>

        <ChevronRight
          size={16}
          className="shrink-0 text-white/30 transition-colors group-hover:text-white/70"
        />
      </span>
    </button>
  );
}
