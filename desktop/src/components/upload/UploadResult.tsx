import { useNavigate } from 'react-router-dom';
import type { UploadedTrack } from '../../lib/upload';
import { useTrackMenuStore } from '../../stores/track-menu';

export interface UploadResultProps {
  result: UploadedTrack;
  onReset: () => void;
}

/** Success summary: the created track, its permalink, and the processing note. */
export function UploadResult({ result, onReset }: UploadResultProps) {
  const navigate = useNavigate();
  const openLink = useTrackMenuStore((s) => s.openLink);
  const permalink = result.permalink_url;
  const trackPath = `/track/${encodeURIComponent(result.urn)}`;

  return (
    <section className="flex flex-col gap-4 border-t border-white/[0.06] pt-6">
      <h2 className="text-[15px] font-semibold text-white/90">{'Upload complete'}</h2>

      <div className="flex flex-col gap-1.5">
        <p className="text-[13.5px] text-white/90">{result.title || 'Uploaded track'}</p>
        {permalink && (
          <button
            type="button"
            title={'Open in the app. Right-click to copy the link.'}
            onClick={() => navigate(trackPath)}
            onContextMenu={(e) => {
              e.preventDefault();
              openLink({ url: permalink, target: trackPath }, e.clientX, e.clientY);
            }}
            className="w-fit cursor-pointer break-all text-left text-[12.5px] text-accent hover:underline"
          >
            {permalink}
          </button>
        )}
      </div>

      {result.state === 'processing' && (
        <p className="text-[12px] leading-relaxed text-white/45">
          {
            'SoundCloud is still processing this track. It can take a little while before it appears publicly.'
          }
        </p>
      )}

      {result.artwork_error && (
        <p role="alert" className="text-[12px] leading-snug text-red-400">
          {'The track was uploaded, but the artwork could not be applied: '}
          {result.artwork_error}
        </p>
      )}

      <div>
        <button
          type="button"
          onClick={onReset}
          className="cursor-pointer rounded-md border border-white/[0.06] bg-white/[0.06] px-4 py-2 text-[12.5px] font-semibold text-white/80 transition-colors hover:bg-white/[0.1]"
        >
          {'Upload another track'}
        </button>
      </div>
    </section>
  );
}
