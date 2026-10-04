import { memo } from 'react';
import { AudioLines } from '../../lib/icons';

/** The login hero — a flat accent logo above the wordmark. */
export const BrandMark = memo(function BrandMark({ subtitle }: { subtitle: string }) {
  return (
    <div className="flex flex-col items-center text-center">
      <div className="relative w-[84px] h-[84px] mb-6">
        {/* logo tile */}
        <div
          className="relative w-full h-full rounded-[26px] flex items-center justify-center"
          style={{
            background: 'var(--color-accent)',
          }}
        >
          <AudioLines size={36} strokeWidth={2} style={{ color: 'var(--color-accent-contrast)' }} />
        </div>
      </div>

      <h1 className="text-[32px] font-black tracking-tight leading-none text-white">SoundCloud</h1>
      <p className="text-[13px] text-white/40 mt-2.5 min-h-[18px]">{subtitle}</p>
    </div>
  );
});
