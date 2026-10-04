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
    <nav className="w-[212px] shrink-0 hidden md:block">
      <div
        className="sticky top-8 rounded-[1.75rem] p-2.5"
        style={{
          border: '0.5px solid rgba(255,255,255,0.07)',
          background: '#141417',
        }}
      >
        <div className="flex flex-col gap-1">
          {categories.map((c) => {
            const on = c.id === active;
            return (
              <button
                key={c.id}
                type="button"
                onClick={() => onChange(c.id)}
                className={`group relative flex items-center gap-3 h-11 pl-3.5 pr-3 rounded-2xl text-[13.5px] font-semibold text-left transition-all duration-200 cursor-pointer ${
                  on ? 'text-white' : 'text-white/45 hover:text-white/85 hover:bg-white/[0.05]'
                }`}
                style={
                  on
                    ? {
                        background: 'rgba(255,255,255,0.08)',
                      }
                    : undefined
                }
              >
                {on && (
                  <span
                    aria-hidden
                    className="absolute left-0 top-1/2 -translate-y-1/2 h-5 w-[3px] rounded-full"
                    style={{
                      background: 'var(--color-accent)',
                      boxShadow: '0 0 10px var(--color-accent)',
                    }}
                  />
                )}
                <span
                  className={`transition-colors duration-200 ${
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
      </div>
    </nav>
  );
}
