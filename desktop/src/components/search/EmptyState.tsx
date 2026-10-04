import { memo } from 'react';

interface EmptyStateProps {
  icon: React.ReactNode;
  title: string;
  body: string;
  /** Optional CTA — omit on the landing fallback (the lens is right above). */
  cta?: string;
  ctaIcon?: React.ReactNode;
  onAction?: () => void;
}

/* One big inviting glass plaque — used wherever the wall would otherwise be
 * blank (landing/dive/text/vibe). Never a gray "no results". */
export const EmptyState = memo(function EmptyState({
  icon,
  title,
  body,
  cta,
  ctaIcon,
  onAction,
}: EmptyStateProps) {
  return (
    <div className="flex justify-center px-4 pt-14">
      <div
        className="w-full max-w-[460px] flex flex-col items-center gap-5 p-10 rounded-[2.25rem] text-center"
        style={{
          background: 'rgba(20,20,24,0.9)',
          border: '0.5px solid rgba(255,255,255,0.1)',
          boxShadow: '0 30px 80px rgba(0,0,0,0.4), inset 0 1px 0 rgba(255,255,255,0.06)',
          isolation: 'isolate',
        }}
      >
        <div
          className="w-16 h-16 flex items-center justify-center rounded-2xl text-accent"
          style={{
            background: 'rgba(255,255,255,0.04)',
            border: '0.5px solid rgba(255,255,255,0.08)',
          }}
        >
          {icon}
        </div>
        <div className="flex flex-col gap-1.5">
          <p className="text-lg font-bold text-white/90">{title}</p>
          <p className="text-[13px] leading-relaxed text-white/45">{body}</p>
        </div>
        {cta && onAction && (
          <button
            type="button"
            onClick={onAction}
            className="inline-flex items-center gap-2 h-11 px-6 rounded-full text-[13px] font-semibold cursor-pointer transition-transform duration-500 hover:scale-[1.03] active:scale-[0.97]"
            style={{
              color: 'var(--color-accent-contrast)',
              background: 'var(--color-accent)',
            }}
          >
            {ctaIcon}
            {cta}
          </button>
        )}
      </div>
    </div>
  );
});
