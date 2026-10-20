import type { CSSProperties, ReactNode } from 'react';

/** Shared row geometry for every sidebar entry (nav, pins, account). */
export const ROW =
  'group relative w-full flex items-center h-10 rounded-xl transition-all duration-200';

const LABEL_T = 'max-width 320ms cubic-bezier(0.2,0.8,0.2,1), opacity 240ms ease';

// Active = white text; the shared sliding indicator supplies the wash.
export const ACTIVE: CSSProperties = {
  color: '#fff',
};

/** A label that always exists but folds away purely via CSS on collapse — no JS
 *  mount/unmount, so the sidebar width + labels glide together. */
export function Label({
  collapsed,
  children,
  className,
}: {
  collapsed: boolean;
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={`overflow-hidden whitespace-nowrap ${className ?? ''}`}
      style={{ maxWidth: collapsed ? 0 : '142px', opacity: collapsed ? 0 : 1, transition: LABEL_T }}
    >
      {children}
    </span>
  );
}

export function IconBox({ children }: { children: ReactNode }) {
  return <span className="w-10 shrink-0 flex items-center justify-center">{children}</span>;
}
