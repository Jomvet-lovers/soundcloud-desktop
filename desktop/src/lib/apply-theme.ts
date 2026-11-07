/**
 * Shared theme/perf var application: accent + background vars and perf gating,
 * written onto the app root by the ThemeProvider.
 */

function hexToRgb(hex: string): string {
  const h = hex.replace('#', '');
  const r = Number.parseInt(h.substring(0, 2), 16);
  const g = Number.parseInt(h.substring(2, 4), 16);
  const b = Number.parseInt(h.substring(4, 6), 16);
  return `${r}, ${g}, ${b}`;
}

/** Write the accent CSS variables (+contrast) onto a root element. */
export function applyAccentVars(accentColor: string, root: HTMLElement = document.documentElement) {
  const rgb = hexToRgb(accentColor);
  root.style.setProperty('--color-accent', accentColor);
  const r = Number.parseInt(accentColor.slice(1, 3), 16);
  const g = Number.parseInt(accentColor.slice(3, 5), 16);
  const b = Number.parseInt(accentColor.slice(5, 7), 16);
  // Hover deepens the accent rather than lightening it (Apple's controls get
  // darker under the pointer), but a dark accent has to lift instead or it
  // would sink into the surface.
  const lum = (0.299 * r + 0.587 * g + 0.114 * b) / 255;
  const step = lum > 0.55 ? -24 : 24;
  const shift = (c: number) => Math.max(0, Math.min(255, c + step));
  root.style.setProperty('--color-accent-hover', `rgb(${shift(r)}, ${shift(g)}, ${shift(b)})`);
  root.style.setProperty('--color-accent-glow', `rgba(${rgb}, 0.2)`);
  root.style.setProperty('--color-accent-selection', `rgba(${rgb}, 0.3)`);
  root.style.setProperty('--color-accent-contrast', lum > 0.62 ? '#000000' : '#ffffff');
}

/** Write the background CSS variables onto a root element. */
export function applyBgVars(bgPrimary: string, root: HTMLElement = document.documentElement) {
  root.style.setProperty('--bg-primary', bgPrimary);
  root.style.setProperty('--bg-titlebar', `rgba(${hexToRgb(bgPrimary)}, 0.95)`);
}

/** Drives index.css `[data-perf="…"]` rules (glass blur radii, idle-animation gates). */
export function applyPerfMode(perfMode: string, root: HTMLElement = document.documentElement) {
  root.dataset.perf = perfMode;
}

/**
 * Surface polarity. `html[data-theme="light"]` flips the neutral scale in
 * index.css (white-alpha utilities become ink), so components keep using
 * `text-white/60`-style classes and still read correctly on a light surface.
 * `color-scheme` hands the same answer to native chrome (scrollbars, form
 * controls, the colour picker).
 */
export function applyThemeMode(mode: string, root: HTMLElement = document.documentElement) {
  root.dataset.theme = mode;
  root.style.colorScheme = mode === 'light' ? 'light' : 'dark';
}
