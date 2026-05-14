export type Gradient = readonly [string, string, string];

const PALETTES: ReadonlyArray<Gradient> = [
  ['#7c3aed', '#06b6d4', '#ec4899'],
  ['#ff5500', '#ff0080', '#ff8a00'],
  ['#06b6d4', '#3b82f6', '#10b981'],
  ['#3f3f46', '#52525b', '#71717a'],
  ['#f97316', '#fb7185', '#a855f7'],
  ['#10b981', '#84cc16', '#065f46'],
  ['#0ea5e9', '#06b6d4', '#1e3a8a'],
  ['#a855f7', '#d946ef', '#f472b6'],
  ['#facc15', '#f97316', '#dc2626'],
  ['#22d3ee', '#a78bfa', '#f0abfc'],
];

const hashString = (s: string): number => {
  let h = 2166136261;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 16777619);
  }
  return h >>> 0;
};

export const gradientForId = (id: string, offset = 0): Gradient =>
  PALETTES[(hashString(id) + offset) % PALETTES.length];

export const monogramOf = (s: string): string => {
  const trimmed = s.trim();
  if (!trimmed) return '?';
  const parts = trimmed.split(/\s+/).slice(0, 2);
  return parts.map((p) => p.charAt(0).toUpperCase()).join('');
};
