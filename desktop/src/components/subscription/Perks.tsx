import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { AudioLines, Database, Globe, Heart, Music, Sparkles } from '../../lib/icons';

const PERKS = [
  { key: 'goPlus', Icon: Music },
  { key: 'server', Icon: Database },
  { key: 'hq', Icon: AudioLines },
  { key: 'whitelist', Icon: Globe },
  { key: 'soundwave', Icon: Sparkles },
  { key: 'support', Icon: Heart },
] as const;

/** Compact perk grid — icon + short title + one-line tag. No marketing paragraphs;
 * scales from a single column (narrow / vertical) up to three (wide). */
export const Perks = memo(function Perks() {
  const { t } = useTranslation();
  return (
    <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2 lg:grid-cols-3">
      {PERKS.map(({ key, Icon }) => (
        <div
          key={key}
          className="flex items-center gap-3.5 rounded-[14px] border border-white/[0.06] px-4 py-3.5 transition-colors duration-300 hover:bg-white/[0.03]"
          style={{ background: 'rgba(255,255,255,0.02)' }}
        >
          <span
            className="grid size-9 shrink-0 place-items-center rounded-[10px] text-accent"
            style={{
              border: '1px solid color-mix(in srgb, var(--color-accent) 28%, transparent)',
              background: 'color-mix(in srgb, var(--color-accent) 9%, transparent)',
            }}
          >
            <Icon size={16} />
          </span>
          <div className="min-w-0">
            <h4 className="truncate text-[14px] font-semibold text-white/90">
              {t(`starpass.perk.${key}.title`)}
            </h4>
            <p className="truncate text-[12px] text-white/45">{t(`starpass.perk.${key}.tag`)}</p>
          </div>
        </div>
      ))}
    </div>
  );
});
