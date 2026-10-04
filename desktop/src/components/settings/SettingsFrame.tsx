import { memo, type ReactNode } from 'react';
import type { Aura } from '../../lib/aura';

/** Flat shell for Settings — decorative atmosphere layers removed. */
export const SettingsFrame = memo(function SettingsFrame({
  children,
}: {
  aura: Aura;
  children: ReactNode;
}) {
  return (
    <div className="relative min-h-full w-full">
      <div className="relative z-10">{children}</div>
    </div>
  );
});
