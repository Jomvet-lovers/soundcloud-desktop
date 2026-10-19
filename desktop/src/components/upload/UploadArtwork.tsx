import { X } from '../../lib/icons';

export interface ArtworkSelection {
  path: string;
  filename: string;
  previewUrl: string | null;
}

export interface UploadArtworkProps {
  artwork: ArtworkSelection | null;
  /** The preview grant failed to load; fall back to the filename only. */
  previewFailed: boolean;
  onPick: () => void;
  onRemove: () => void;
  onPreviewError: () => void;
}

/** Optional artwork picker. The preview reads through the same one-time
 *  loopback grant the upload uses; submit mints a fresh token right before. */
export function UploadArtwork({
  artwork,
  previewFailed,
  onPick,
  onRemove,
  onPreviewError,
}: UploadArtworkProps) {
  return (
    <div className="flex flex-col gap-3 border-t border-white/[0.06] pt-5">
      <div className="flex items-center justify-between gap-4">
        <div className="min-w-0">
          <p className="text-[13.5px] font-medium text-white/80">{'Artwork'}</p>
          <p className="mt-0.5 text-[11.5px] leading-snug text-white/35">
            {'Optional cover image (JPG, PNG, or WebP).'}
          </p>
        </div>
        <button
          type="button"
          onClick={onPick}
          className="shrink-0 cursor-pointer rounded-lg border border-white/[0.08] bg-white/[0.04] px-3 py-1.5 text-[12px] font-medium text-white/70 transition-colors hover:bg-white/[0.08] hover:text-white/90"
        >
          {'Choose artwork'}
        </button>
      </div>

      {artwork && (
        <div className="flex items-center gap-3">
          {artwork.previewUrl && !previewFailed ? (
            <img
              src={artwork.previewUrl}
              alt=""
              onError={onPreviewError}
              className="size-12 shrink-0 rounded object-cover"
            />
          ) : null}
          <p className="min-w-0 flex-1 truncate text-[12.5px] text-white/70">{artwork.filename}</p>
          <button
            type="button"
            onClick={onRemove}
            title={'Remove artwork'}
            aria-label={'Remove artwork'}
            className="shrink-0 cursor-pointer rounded p-1.5 text-white/40 transition-colors hover:bg-white/[0.06] hover:text-white/80"
          >
            <X size={14} />
          </button>
        </div>
      )}
    </div>
  );
}
