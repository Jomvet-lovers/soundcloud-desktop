import { useMemo } from 'react';
import { useSettingsStore } from '../stores/settings';
import { type Aura, auraFromHex, DEFAULT_AURA } from './aura';

/**
 * Default aura for profiles without a "star": built from the viewer's (current
 * user's) accent color so a default profile matches their theme. Profiles with
 * a "star" use the color picked by the owner — this hook isn't involved there.
 */
export function useViewerAura(): Aura {
  const accentColor = useSettingsStore((s) => s.accentColor);
  return useMemo(() => auraFromHex(accentColor) ?? DEFAULT_AURA, [accentColor]);
}
