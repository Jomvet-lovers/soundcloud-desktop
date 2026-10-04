import { memo, type ReactNode } from 'react';
import { LIBRARY_KEYFRAMES } from './keyframes';
import type { Soundprint } from './useSoundprint';

/** Shared shell for every Library surface — flat content column. */
export const LibraryFrame = memo(function LibraryFrame({
  children,
}: {
  sound: Soundprint;
  children: ReactNode;
}) {
  return (
    <div className="relative min-h-full w-full">
      <style>{LIBRARY_KEYFRAMES}</style>
      <div
        className="relative z-10 min-h-full max-w-[1320px] mx-auto px-4 md:px-8 pt-5 pb-6"
        style={{ isolation: 'isolate' }}
      >
        {children}
      </div>
    </div>
  );
});
