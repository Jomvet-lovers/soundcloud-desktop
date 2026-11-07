import { useEffect } from 'react';
import { applyAccentVars, applyBgVars, applyPerfMode, applyThemeMode } from '../lib/apply-theme';
import { setupVisibilityGate } from '../lib/perf';
import { THEME_MODES, useSettingsStore } from '../stores/settings';

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const accentColor = useSettingsStore((s) => s.accentColor);
  const perfMode = useSettingsStore((s) => s.perfMode);
  const themeMode = useSettingsStore((s) => s.themeMode);
  const bgPrimary = THEME_MODES[themeMode].bg;

  // One global gate that pauses every CSS animation while the window is hidden
  // (the WebView does not throttle timers/rAF). Install once.
  useEffect(() => {
    setupVisibilityGate();
  }, []);

  // Drives index.css `[data-perf="…"]` rules (glass blur radii, idle-animation gates).
  useEffect(() => {
    applyPerfMode(perfMode);
  }, [perfMode]);

  // Drives index.css `[data-theme="light"]` (neutral-scale flip) + color-scheme.
  useEffect(() => {
    applyThemeMode(themeMode);
  }, [themeMode]);

  useEffect(() => {
    applyAccentVars(accentColor);
  }, [accentColor]);

  useEffect(() => {
    applyBgVars(bgPrimary);
    document.documentElement.style.backgroundColor = bgPrimary;
  }, [bgPrimary]);

  return <>{children}</>;
}
