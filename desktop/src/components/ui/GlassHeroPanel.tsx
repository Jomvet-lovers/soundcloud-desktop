import type React from 'react';
import { memo } from 'react';
import type { Aura } from '../../lib/aura';

interface GlassHeroPanelProps {
  hasStar: boolean;
  aura: Aura;
  className?: string;
  children: React.ReactNode;
}

function GlassHeroPanelImpl(props: GlassHeroPanelProps) {
  const { className, children } = props;
  return (
    <div
      className={`relative rounded-3xl border border-white/[0.08] bg-[#141417] ${className ?? ''}`}
    >
      {children}
    </div>
  );
}

export const GlassHeroPanel = memo(GlassHeroPanelImpl);
