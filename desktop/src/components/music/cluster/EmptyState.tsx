import React from 'react';
import { AudioLines } from '../../../lib/icons';

interface Props {
  title: string;
  description: string;
  icon?: React.ReactNode;
}

export const EmptyState = React.memo(function EmptyState({ title, description, icon }: Props) {
  return (
    <div className="py-14 flex flex-col items-center gap-4 text-center">
      <div
        className="w-16 h-16 rounded-2xl flex items-center justify-center"
        style={{
          background: 'rgba(255,255,255,0.04)',
          border: '0.5px solid rgba(255,255,255,0.08)',
        }}
      >
        {icon ?? <AudioLines size={22} className="text-white/25" />}
      </div>
      <div className="space-y-1.5 max-w-sm">
        <p className="text-[13px] font-semibold text-white/70">{title}</p>
        <p className="text-[11.5px] text-white/35 leading-relaxed">{description}</p>
      </div>
    </div>
  );
});
