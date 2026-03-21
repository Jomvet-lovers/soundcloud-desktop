import { useEffect } from 'react';
import { useSettingsStore } from '../stores/settings';

function hexToRgb(hex: string): string {
  const h = hex.replace('#', '');
  const r = Number.parseInt(h.substring(0, 2), 16);
  const g = Number.parseInt(h.substring(2, 4), 16);
  const b = Number.parseInt(h.substring(4, 6), 16);
  return `${r}, ${g}, ${b}`;
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const accentColor = useSettingsStore((s) => s.accentColor);
  const bgPrimary = useSettingsStore((s) => s.bgPrimary);

  useEffect(() => {
    const root = document.documentElement;
    const rgb = hexToRgb(accentColor);
    root.style.setProperty('--color-accent', accentColor);
    const r = Number.parseInt(accentColor.slice(1, 3), 16);
    const g = Number.parseInt(accentColor.slice(3, 5), 16);
    const b = Number.parseInt(accentColor.slice(5, 7), 16);
    const hover = `rgb(${Math.min(255, r + 26)}, ${Math.min(255, g + 26)}, ${Math.min(255, b + 26)})`;
    root.style.setProperty('--color-accent-hover', hover);
    root.style.setProperty('--color-accent-glow', `rgba(${rgb}, 0.2)`);
    root.style.setProperty('--color-accent-selection', `rgba(${rgb}, 0.3)`);
    // Contrast: black text on light accent, white on dark
    const lum = 0.299 * r + 0.587 * g + 0.114 * b;
    root.style.setProperty('--color-accent-contrast', lum > 160 ? '#000000' : '#ffffff');
  }, [accentColor]);

  useEffect(() => {
    const root = document.documentElement;
    root.style.setProperty('--bg-primary', bgPrimary);
    const bgRgb = hexToRgb(bgPrimary);
    root.style.setProperty('--bg-titlebar', `rgba(${bgRgb}, 0.95)`);
    root.style.backgroundColor = bgPrimary;
  }, [bgPrimary]);

  return <>{children}</>;
}
