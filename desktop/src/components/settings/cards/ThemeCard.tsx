import { useRef } from 'react';
import { Sparkles } from '../../../lib/icons';
import {
  ACCENT_OPTIONS,
  THEME_MODES,
  type ThemeMode,
  useSettingsStore,
} from '../../../stores/settings';
import { Card } from '../primitives';

const THEME_MODE_KEYS: ThemeMode[] = ['dark', 'light'];

const THEME_MODE_LABEL: Record<ThemeMode, string> = {
  dark: 'Dark',
  light: 'Light',
};

export function ThemeCard() {
  const accentColor = useSettingsStore((s) => s.accentColor);
  const themeMode = useSettingsStore((s) => s.themeMode);
  const setAccentColor = useSettingsStore((s) => s.setAccentColor);
  const setThemeMode = useSettingsStore((s) => s.setThemeMode);
  const resetTheme = useSettingsStore((s) => s.resetTheme);
  const colorInputRef = useRef<HTMLInputElement>(null);

  const isCustomAccent = !ACCENT_OPTIONS.some(
    (option) => option.color.toLowerCase() === accentColor.toLowerCase(),
  );

  return (
    <Card
      title={'Appearance'}
      icon={<Sparkles size={17} />}
      action={
        <button
          type="button"
          onClick={resetTheme}
          className="text-[12px] text-white/30 hover:text-white/60 transition-colors cursor-pointer"
        >
          {'Reset to defaults'}
        </button>
      }
    >
      <div className="space-y-6">
        <div className="space-y-3">
          <label className="text-[13px] text-white/50 font-medium">{'Theme'}</label>
          <div className="grid grid-cols-2 gap-3">
            {THEME_MODE_KEYS.map((id) => {
              const def = THEME_MODES[id];
              const isActive = themeMode === id;
              return (
                <button
                  key={id}
                  type="button"
                  onClick={() => setThemeMode(id)}
                  className={`group relative rounded-2xl overflow-hidden border transition-all duration-200 cursor-pointer hover:scale-[1.02] active:scale-[0.98] ${
                    isActive
                      ? 'border-white/30 ring-1 ring-white/20'
                      : 'border-white/[0.06] hover:border-white/15'
                  }`}
                >
                  <div
                    className="relative h-20 overflow-hidden"
                    style={{ backgroundColor: def.bg }}
                  >
                    {/* A miniature of the app: recessed sidebar + content rows. */}
                    <div
                      className="absolute inset-y-0 left-0 w-1/4"
                      style={{ backgroundColor: def.bgChrome }}
                    />
                    <div className="absolute left-[32%] right-3 top-3 flex items-center gap-2">
                      <div
                        className="w-4 h-4 rounded-full"
                        style={{ backgroundColor: accentColor }}
                      />
                      <div
                        className="h-1.5 flex-1 rounded-full"
                        style={{ backgroundColor: def.preview[2] }}
                      />
                    </div>
                    <div className="absolute left-[32%] right-3 top-8 space-y-1.5">
                      <div
                        className="h-1.5 w-3/4 rounded-full"
                        style={{ backgroundColor: def.preview[2] }}
                      />
                      <div
                        className="h-1.5 w-1/2 rounded-full"
                        style={{ backgroundColor: def.preview[2] }}
                      />
                    </div>
                    <div
                      className="absolute bottom-2 right-2 w-6 h-6 rounded-md"
                      style={{ backgroundColor: def.preview[2] }}
                    />
                  </div>
                  <div className="px-3 py-2 bg-white/[0.03] text-center">
                    <span
                      className={`text-[12px] font-medium ${isActive ? 'text-white/90' : 'text-white/50'}`}
                    >
                      {THEME_MODE_LABEL[id]}
                    </span>
                  </div>
                </button>
              );
            })}
          </div>
        </div>

        <div className="space-y-3">
          <label className="text-[13px] text-white/50 font-medium">{'Accent color'}</label>
          <div className="flex items-center gap-2 flex-wrap">
            {ACCENT_OPTIONS.map((option) => {
              const isActive = accentColor.toLowerCase() === option.color.toLowerCase();
              return (
                <button
                  key={option.id}
                  type="button"
                  title={option.label}
                  aria-label={option.label}
                  aria-pressed={isActive}
                  onClick={() => setAccentColor(option.color)}
                  className="w-8 h-8 rounded-full border-2 transition-all duration-200 cursor-pointer hover:scale-110 active:scale-95 shadow-md"
                  style={{
                    backgroundColor: option.color,
                    borderColor: isActive ? 'var(--ov-ink)' : 'transparent',
                    boxShadow: isActive ? `0 0 16px ${option.color}60` : undefined,
                  }}
                />
              );
            })}
            <button
              type="button"
              title={'Custom color'}
              aria-label={'Custom color'}
              onClick={() => colorInputRef.current?.click()}
              className={`w-8 h-8 rounded-full border-2 transition-all cursor-pointer flex items-center justify-center hover:scale-110 ${
                isCustomAccent
                  ? 'border-white/70'
                  : 'border-dashed border-white/20 text-white/30 hover:border-white/40 hover:text-white/60'
              }`}
              style={isCustomAccent ? { backgroundColor: accentColor } : undefined}
            >
              {isCustomAccent ? null : <span className="text-[11px] font-bold">+</span>}
            </button>
          </div>
        </div>

        <input
          ref={colorInputRef}
          type="color"
          value={accentColor}
          onChange={(e) => setAccentColor(e.target.value)}
          className="sr-only"
        />
      </div>
    </Card>
  );
}
