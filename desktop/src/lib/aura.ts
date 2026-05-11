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

export const AURAS: ReadonlyArray<Aura> = [
  {
    id: 'aurora',
    name: 'Aurora',
    orbs: ['#7c3aed', '#06b6d4', '#ec4899'],
    accent: [168, 85, 247],
    nameGradient:
      'linear-gradient(110deg, #fff 0%, #fff 28%, #c4b5fd 45%, #f0abfc 58%, #fff 75%, #fff 100%)',
  },
  {
    id: 'magma',
    name: 'Magma',
    orbs: ['#ff5500', '#ff0080', '#ff8a00'],
    accent: [255, 85, 0],
    nameGradient:
      'linear-gradient(110deg, #fff 0%, #fff 28%, #ffb38a 45%, #ff5cb1 58%, #fff 75%, #fff 100%)',
  },
  {
    id: 'cyber',
    name: 'Cyber',
    orbs: ['#06b6d4', '#3b82f6', '#10b981'],
    accent: [6, 182, 212],
    nameGradient:
      'linear-gradient(110deg, #fff 0%, #fff 28%, #67e8f9 45%, #93c5fd 58%, #fff 75%, #fff 100%)',
  },
  {
    id: 'void',
    name: 'Void',
    orbs: ['#3f3f46', '#52525b', '#71717a'],
    accent: [212, 212, 220],
    nameGradient: 'linear-gradient(110deg, #fff 0%, #fff 30%, #d4d4d8 50%, #fff 70%, #fff 100%)',
  },
  {
    id: 'sunset',
    name: 'Sunset',
    orbs: ['#f97316', '#fb7185', '#a855f7'],
    accent: [251, 113, 133],
    nameGradient:
      'linear-gradient(110deg, #fff 0%, #fff 28%, #fed7aa 45%, #fda4af 58%, #fff 75%, #fff 100%)',
  },
  {
    id: 'forest',
    name: 'Forest',
    orbs: ['#10b981', '#84cc16', '#065f46'],
    accent: [16, 185, 129],
    nameGradient:
      'linear-gradient(110deg, #fff 0%, #fff 28%, #bbf7d0 45%, #86efac 58%, #fff 75%, #fff 100%)',
  },
  {
    id: 'ocean',
    name: 'Ocean',
    orbs: ['#0ea5e9', '#06b6d4', '#1e3a8a'],
    accent: [14, 165, 233],
    nameGradient:
      'linear-gradient(110deg, #fff 0%, #fff 28%, #bae6fd 45%, #7dd3fc 58%, #fff 75%, #fff 100%)',
  },
];

export const DEFAULT_AURA: Aura = AURAS[0];
export const DEFAULT_CUSTOM_HEX = '#a855f7';

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

const rgbToHex = ([r, g, b]: [number, number, number]) =>
  `#${[r, g, b].map((n) => n.toString(16).padStart(2, '0')).join('')}`;

const lighten = ([r, g, b]: [number, number, number], amt: number): [number, number, number] => [
  Math.min(255, Math.round(r + (255 - r) * amt)),
  Math.min(255, Math.round(g + (255 - g) * amt)),
  Math.min(255, Math.round(b + (255 - b) * amt)),
];

const darken = ([r, g, b]: [number, number, number], amt: number): [number, number, number] => [
  Math.round(r * (1 - amt)),
  Math.round(g * (1 - amt)),
  Math.round(b * (1 - amt)),
];

export const auraFromHex = (hex: string): Aura | null => {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  const c1 = rgbToHex(rgb);
  const c2 = rgbToHex(lighten(rgb, 0.25));
  const c3 = rgbToHex(darken(rgb, 0.35));
  const lighter = rgbToHex(lighten(rgb, 0.5));
  return {
    id: 'custom',
    name: 'Custom',
    orbs: [c1, c2, c3],
    accent: rgb,
    nameGradient: `linear-gradient(110deg, #fff 0%, #fff 28%, ${lighter} 45%, ${c2} 58%, #fff 75%, #fff 100%)`,
  };
};

export const auraById = (id: string): Aura | null => {
  for (const a of AURAS) if (a.id === id) return a;
  return null;
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
