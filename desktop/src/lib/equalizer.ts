/* ── Constants ─────────────────────────────────────────────── */

export const EQ_FREQUENCIES = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000] as const;
export const EQ_LABELS = ['32', '64', '125', '250', '500', '1K', '2K', '4K', '8K', '16K'];
export const EQ_BAND_COUNT = EQ_FREQUENCIES.length;
export const EQ_MIN_GAIN = -12;
export const EQ_MAX_GAIN = 12;

export const EQ_PRESETS: Record<string, { label: string; gains: number[] }> = {
  flat: { label: 'Flat', gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  bassBoost: { label: 'Bass Boost', gains: [6, 5, 4, 2, 0, 0, 0, 0, 0, 0] },
  bassDestroyer: {
    label: 'Bass Destroyer',
    gains: [12, 12, 10, 7, 3, 0, -2, -4, -4, -5],
  },
  trebleBoost: {
    label: 'Treble Boost',
    gains: [0, 0, 0, 0, 0, 0, 2, 4, 5, 6],
  },
  vocal: { label: 'Vocal', gains: [-2, -1, 0, 2, 4, 4, 3, 1, 0, -1] },
  rock: { label: 'Rock', gains: [4, 3, 1, 0, -1, 0, 2, 3, 4, 4] },
  electronic: {
    label: 'Electronic',
    gains: [5, 4, 2, 0, -1, 0, 1, 3, 4, 5],
  },
  classical: {
    label: 'Classical',
    gains: [0, 0, 0, 0, 0, 0, -2, -3, -3, -4],
  },
  loudness: {
    label: 'Loudness',
    gains: [5, 4, 1, 0, -1, 0, -1, 0, 3, 4],
  },
  vShape: {
    label: 'V-Shape',
    gains: [5, 3, 1, -1, -3, -3, -1, 1, 3, 5],
  },
  nightMode: {
    label: 'Night',
    gains: [-3, -2, 0, 2, 3, 3, 2, 0, -2, -4],
  },
};

/** Minimal shape of a user-created profile (the store owns the full record). */
export interface EqProfileShape {
  id: string;
  gains: number[];
}

/** Gains for a profile id: user profiles first, then factory presets with the
 *  user's saved edit applied. Null when the id is unknown. */
export function eqGainsFor(
  id: string,
  profiles: EqProfileShape[],
  edits: Record<string, number[]>,
): number[] | null {
  const user = profiles.find((p) => p.id === id);
  if (user) return [...user.gains];
  const preset = EQ_PRESETS[id];
  if (!preset) return null;
  return [...(edits[id] ?? preset.gains)];
}
