import { durLong, formatBytes } from '../../lib/formatters';
import { AlertCircle, Loader2, Music } from '../../lib/icons';
import type { AudioInspection } from '../../lib/upload';

export interface AudioSelection {
  path: string;
  info: AudioInspection;
}

export interface UploadAudioProps {
  audio: AudioSelection | null;
  inspecting: boolean;
  inspectError: string | null;
  /** True while an upload is in flight: the file cannot be swapped. */
  disabled: boolean;
  onPick: () => void;
}

const FILE_BTN =
  'shrink-0 cursor-pointer rounded-lg border border-white/[0.08] bg-white/[0.04] px-3 py-1.5 text-[12px] font-medium text-white/70 transition-colors hover:bg-white/[0.08] hover:text-white/90 disabled:cursor-not-allowed disabled:opacity-40';

/** File picker plus the `upload_inspect` verdict (warnings and size gates). */
export function UploadAudio({
  audio,
  inspecting,
  inspectError,
  disabled,
  onPick,
}: UploadAudioProps) {
  const tooLarge = audio?.info.too_large ?? false;
  const tooLong = audio?.info.too_long ?? false;

  return (
    <section className="flex flex-col gap-4">
      <h2 className="text-[15px] font-semibold text-white/90">{'Audio file'}</h2>

      {inspecting ? (
        <p className="flex items-center gap-2 text-[13px] text-white/50">
          <Loader2 size={14} className="animate-spin" />
          {'Checking file...'}
        </p>
      ) : audio ? (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <span className="flex size-10 shrink-0 items-center justify-center rounded bg-white/[0.06] text-white/30">
              <Music size={16} />
            </span>
            <div className="min-w-0 flex-1">
              <p className="truncate text-[13.5px] text-white/90">{audio.info.filename}</p>
              <p className="mt-0.5 text-[11.5px] tabular-nums text-white/40">
                {formatBytes(audio.info.size)}
                {audio.info.duration_ms !== null ? ` · ${durLong(audio.info.duration_ms)}` : ''}
              </p>
            </div>
            <button type="button" disabled={disabled} onClick={onPick} className={FILE_BTN}>
              {'Choose another file'}
            </button>
          </div>

          {audio.info.warnings.length > 0 && (
            <ul className="flex flex-col gap-1">
              {audio.info.warnings.map((warning) => (
                <li
                  key={warning}
                  className="flex items-start gap-2 text-[12px] leading-snug text-white/55"
                >
                  <AlertCircle size={13} className="mt-0.5 shrink-0 text-white/30" />
                  <span>{warning}</span>
                </li>
              ))}
            </ul>
          )}

          {tooLong && (
            <p className="flex items-start gap-2 text-[12.5px] leading-snug text-red-400">
              <AlertCircle size={14} className="mt-0.5 shrink-0" />
              {'Files longer than 24 hours cannot be uploaded. Choose a different file.'}
            </p>
          )}

          {tooLarge && (
            <p className="flex items-start gap-2 text-[12.5px] leading-snug text-red-400">
              <AlertCircle size={14} className="mt-0.5 shrink-0" />
              {
                'This file is over the 500 MB limit, so it cannot be uploaded. Choose a different file.'
              }
            </p>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-3">
          <p className="text-[12.5px] leading-relaxed text-white/45">
            {'Pick an audio file, add details and artwork, then upload it to SoundCloud.'}
          </p>
          <button
            type="button"
            onClick={onPick}
            className="flex w-fit cursor-pointer items-center gap-2 rounded-md border border-white/[0.06] bg-white/[0.06] px-4 py-2 text-[12.5px] font-semibold text-white/80 transition-colors hover:bg-white/[0.1]"
          >
            <Music size={14} />
            {'Choose audio file'}
          </button>
        </div>
      )}

      {inspectError && !inspecting && (
        <p className="text-[12px] leading-snug text-red-400">{inspectError}</p>
      )}
    </section>
  );
}
