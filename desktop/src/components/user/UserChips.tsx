import React from 'react';
import { Check, Globe, Instagram, LinkIcon, Twitter, Youtube } from '../../lib/icons';

export const VerifiedBadge = React.memo(function VerifiedBadge({ title }: { title: string }) {
  return (
    <div
      className="w-6 h-6 rounded-full bg-blue-500 flex items-center justify-center ring-2 ring-white/20 shadow-[0_0_16px_rgba(59,130,246,0.55)]"
      title={title}
    >
      <Check size={13} className="text-white" strokeWidth={3.5} />
    </div>
  );
});

export const ProChip = React.memo(function ProChip({ plan }: { plan: string }) {
  return (
    <span
      className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[10px] font-bold uppercase tracking-[0.18em] text-orange-300/90"
      style={{
        background: 'linear-gradient(135deg, rgba(255,85,0,0.18), rgba(255,0,128,0.10))',
        border: '0.5px solid rgba(255,85,0,0.25)',
        backdropFilter: 'blur(12px)',
        WebkitBackdropFilter: 'blur(12px)',
      }}
    >
      <span className="w-1.5 h-1.5 rounded-full bg-orange-400 shadow-[0_0_6px_#ff5500]" />
      {plan}
    </span>
  );
});

export const InfoChip = React.memo(function InfoChip({
  icon,
  children,
}: {
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <span
      className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-full text-[10px] font-semibold uppercase tracking-[0.16em] text-white/55"
      style={{
        background: 'rgba(255,255,255,0.04)',
        border: '0.5px solid rgba(255,255,255,0.07)',
        backdropFilter: 'blur(12px)',
        WebkitBackdropFilter: 'blur(12px)',
      }}
    >
      <span className="text-white/45">{icon}</span>
      {children}
    </span>
  );
});

export function getWebIcon(service: string) {
  switch (service.toLowerCase()) {
    case 'instagram':
      return <Instagram size={14} />;
    case 'twitter':
      return <Twitter size={14} />;
    case 'youtube':
      return <Youtube size={14} />;
    case 'personal':
      return <Globe size={14} />;
    default:
      return <LinkIcon size={14} />;
  }
}
