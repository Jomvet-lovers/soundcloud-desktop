// Warm-metal "foil" derived from the app accent — NOT a rainbow. The gradient
// stays anchored to --color-accent (color-mix), so it reskins with the user's
// accent and reads as holographic foil over dark glass at low opacity + screen.
export const FOIL_GRADIENT =
  'linear-gradient(120deg,' +
  ' color-mix(in srgb, var(--color-accent) 70%, #fff) 0%,' +
  ' var(--color-accent) 30%,' +
  ' color-mix(in srgb, var(--color-accent) 55%, #000) 52%,' +
  ' color-mix(in srgb, var(--color-accent) 85%, #fff) 72%,' +
  ' var(--color-accent) 100%)';
