import { Loader2, X } from '../../lib/icons';

export interface UploadProgressProps {
  /** Last progress sample; null until the first event arrives. */
  percent: number | null;
  cancelling: boolean;
  onCancel: () => void;
}

/** In-flight upload: status line, progress bar, cancel control. */
export function UploadProgress({ percent, cancelling, onCancel }: UploadProgressProps) {
  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-4 text-[12.5px]">
        <span className="flex items-center gap-2 text-white/60">
          <Loader2 size={13} className="animate-spin" />
          {percent === null ? 'Preparing...' : 'Uploading...'}
        </span>
        <span className="tabular-nums text-white/40">{percent === null ? '' : `${percent}%`}</span>
      </div>

      <div
        role="progressbar"
        aria-label={'Upload progress'}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
        className="h-1.5 overflow-hidden rounded-full bg-white/[0.06]"
      >
        <div
          className="h-full bg-[var(--color-accent)] transition-[width] duration-300"
          style={{ width: `${percent ?? 0}%` }}
        />
      </div>

      <div>
        <button
          type="button"
          disabled={cancelling}
          onClick={onCancel}
          className="flex cursor-pointer items-center gap-2 rounded-md border border-white/[0.06] bg-white/[0.04] px-4 py-2 text-[12px] font-semibold text-white/60 transition-all duration-200 hover:border-white/[0.12] hover:bg-white/[0.08] disabled:cursor-not-allowed disabled:opacity-50"
        >
          <X size={12} />
          {cancelling ? 'Stopping...' : 'Cancel'}
        </button>
      </div>
    </div>
  );
}
