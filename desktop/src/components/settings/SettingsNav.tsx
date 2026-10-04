import { useTranslation } from 'react-i18next';
import type { SettingsCategory, SettingsCategoryId } from './registry';

/** Left rail — a sticky frosted panel of category pills, lit by the accent. */
export function SettingsNav({
  categories,
  active,
  onChange,
}: {
  categories: SettingsCategory[];
  active: SettingsCategoryId;
  onChange: (id: SettingsCategoryId) => void;
}) {
  const { t } = useTranslation();
  return (
    <nav className="w-[200px] shrink-0 hidden md:block">
      <div className="sticky top-8 flex flex-col gap-0.5">
        {categories.map((c) => {
          const on = c.id === active;
          return (
            <button
              key={c.id}
              type="button"
              onClick={() => onChange(c.id)}
              className={`group relative flex items-center gap-3 h-10 pl-3 pr-3 rounded-md text-[13px] font-medium text-left transition-colors cursor-pointer ${
                on
                  ? 'text-white bg-white/[0.06]'
                  : 'text-white/45 hover:text-white/80 hover:bg-white/[0.04]'
              }`}
            >
              <span
                className={`transition-colors ${
                  on ? 'text-[var(--color-accent)]' : 'text-white/40 group-hover:text-white/70'
                }`}
              >
                {c.icon}
              </span>
              {t(c.labelKey)}
            </button>
          );
        })}
      </div>
    </nav>
  );
}
