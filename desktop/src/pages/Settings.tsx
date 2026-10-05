import { useState } from 'react';
import { SETTINGS_CATEGORIES, type SettingsCategoryId } from '../components/settings/registry';
import { SettingsFrame } from '../components/settings/SettingsFrame';
import { SettingsNav } from '../components/settings/SettingsNav';
import { useViewerAura } from '../lib/useViewerAura';

/** Settings — two-pane workspace: a flat category rail on the left, the active
 *  category's sections on the right. */
export function Settings() {
  const aura = useViewerAura();
  const [active, setActive] = useState<SettingsCategoryId>('general');
  const category = SETTINGS_CATEGORIES.find((c) => c.id === active) ?? SETTINGS_CATEGORIES[0];
  const Body = category.Body;

  return (
    <SettingsFrame aura={aura}>
      <div className="max-w-[1080px] mx-auto px-6 md:px-8 pt-8 pb-32 flex gap-8">
        <SettingsNav categories={SETTINGS_CATEGORIES} active={active} onChange={setActive} />
        <div className="flex-1 min-w-0">
          <header className="mb-7 flex items-center gap-3">
            <span className="shrink-0 text-[var(--color-accent)]">{category.icon}</span>
            <div className="min-w-0">
              <p className="text-[11px] text-white/35 font-medium mb-1">{'Settings'}</p>
              <h1 className="text-[26px] font-bold tracking-tight leading-none text-white">
                {category.label}
              </h1>
            </div>
          </header>
          <div key={active} className="space-y-8 animate-fade-in-up">
            <Body />
          </div>
        </div>
      </div>
    </SettingsFrame>
  );
}
