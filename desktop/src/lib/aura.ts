// Aura palettes were part of the removed "vibe" layer. The API is kept so
// existing consumers compile, but every palette is neutral (no hues, no
// gradients).

export type AuraId =
  | 'aurora'
  | 'magma'
  | 'cyber'
  | 'void'
  | 'sunset'
  | 'forest'
  | 'ocean'
  | 'custom';

export type Aura = {
  id: AuraId;
  name: string;
  orbs: [string, string, string];
  accent: [number, number, number];
  nameGradient: string;
};

const NEUTRAL_ORBS: [string, string, string] = ['#3f3f46', '#52525b', '#71717a'];
const NEUTRAL_ACCENT: [number, number, number] = [212, 212, 220];

const neutral = (id: AuraId, name: string): Aura => ({
  id,
  name,
  orbs: NEUTRAL_ORBS,
  accent: NEUTRAL_ACCENT,
  nameGradient: 'none',
});

export const AURAS: ReadonlyArray<Aura> = [neutral('void', 'Neutral')];

export const DEFAULT_AURA: Aura = AURAS[0];
export const DEFAULT_CUSTOM_HEX = '#9ca3af';

export const auraRgba = (a: Aura, alpha: number) =>
  `rgba(${a.accent[0]}, ${a.accent[1]}, ${a.accent[2]}, ${alpha})`;

export const auraRgb = (a: Aura) => `rgb(${a.accent[0]}, ${a.accent[1]}, ${a.accent[2]})`;

const luminance = ([r, g, b]: [number, number, number]) =>
  (0.299 * r + 0.587 * g + 0.114 * b) / 255;

export const isLight = (a: Aura) => luminance(a.accent) > 0.78;

export const hexToRgb = (hex: string): [number, number, number] | null => {
  const m = hex.replace('#', '').match(/^([0-9a-f]{6})$/i);
  if (!m) return null;
  const v = parseInt(m[1], 16);
  return [(v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff];
};

export const auraFromHex = (hex: string): Aura | null => {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  return {
    id: 'custom',
    name: 'Custom',
    orbs: NEUTRAL_ORBS,
    accent: rgb,
    nameGradient: 'none',
  };
};

export const auraById = (id: string): Aura | null => {
  for (const a of AURAS) if (a.id === id) return a;
  return neutral('void', 'Neutral');
};

export const resolveAura = (
  auraId: string | null | undefined,
  customHex: string | null | undefined,
): Aura => {
  if (!auraId) return DEFAULT_AURA;
  if (auraId === 'custom' && customHex) {
    const a = auraFromHex(customHex);
    if (a) return a;
  }
  return auraById(auraId) ?? DEFAULT_AURA;
};
