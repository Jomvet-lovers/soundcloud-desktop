import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { type ArtworkSelection, UploadArtwork } from '../components/upload/UploadArtwork';
import { type AudioSelection, UploadAudio } from '../components/upload/UploadAudio';
import { UploadForm } from '../components/upload/UploadForm';
import { UploadProgress } from '../components/upload/UploadProgress';
import { UploadResult } from '../components/upload/UploadResult';
import { describeError } from '../lib/diagnostics';
import {
  cancelUpload,
  inspectAudioFile,
  issueUploadToken,
  startUpload,
  type UploadedTrack,
  type UploadMeta,
} from '../lib/upload';
import { useAuthStore } from '../stores/auth';

const AUDIO_EXTENSIONS = [
  'mp3',
  'm4a',
  'aac',
  'flac',
  'wav',
  'ogg',
  'opus',
  'aiff',
  'aif',
  'wma',
  'mp2',
  'amr',
];
const ARTWORK_EXTENSIONS = ['jpg', 'jpeg', 'png', 'webp'];

type Phase = 'idle' | 'uploading' | 'done' | 'error' | 'cancelled';

const INITIAL_META: UploadMeta = {
  title: '',
  sharing: 'private',
  embeddable_by: 'all',
  downloadable: true,
  commentable: true,
};

/** Strip the extension so the filename becomes a usable default title. */
function titleFromFilename(filename: string): string {
  return filename.replace(/\.[^.]+$/, '') || filename;
}

/** `crypto.randomUUID` needs a secure context; the fallback stays inside the
 *  id charset Rust validates ([A-Za-z0-9_-], 64 chars max). */
function newUploadId(): string {
  if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/** Upload — pick an audio file, fill in metadata, POST it to SoundCloud. */
export function Upload() {
  const username = useAuthStore((s) => s.user?.username ?? '');
  const [audio, setAudio] = useState<AudioSelection | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [artwork, setArtwork] = useState<ArtworkSelection | null>(null);
  const [previewFailed, setPreviewFailed] = useState(false);
  const [largeConfirmed, setLargeConfirmed] = useState(false);
  const [meta, setMeta] = useState<UploadMeta>(INITIAL_META);
  const [phase, setPhase] = useState<Phase>('idle');
  const [progress, setProgress] = useState<number | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [activeUploadId, setActiveUploadId] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [result, setResult] = useState<UploadedTrack | null>(null);
  const cancelRequested = useRef(false);

  const uploading = phase === 'uploading';
  const info = audio?.info ?? null;
  const blocked = info?.too_long ?? false;
  const submitDisabled = (info?.too_large ?? false) && !largeConfirmed;

  const pickAudio = useCallback(async () => {
    if (inspecting || uploading) return;
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      multiple: false,
      filters: [{ name: 'Audio', extensions: AUDIO_EXTENSIONS }],
    });
    if (typeof picked !== 'string') return;
    setInspecting(true);
    setInspectError(null);
    try {
      const inspection = await inspectAudioFile(picked);
      setAudio({ path: picked, info: inspection });
      setLargeConfirmed(false);
      setResult(null);
      setPhase('idle');
      setFailure(null);
      setProgress(null);
      setMeta((prev) => ({
        ...prev,
        title: titleFromFilename(inspection.filename),
        artist: prev.artist?.trim() ? prev.artist : username,
      }));
    } catch (err) {
      const message = describeError(err);
      setInspectError(message);
      toast.error(message);
    } finally {
      setInspecting(false);
    }
  }, [inspecting, uploading, username]);

  const pickArtwork = useCallback(async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({
      multiple: false,
      filters: [{ name: 'Image', extensions: ARTWORK_EXTENSIONS }],
    });
    if (typeof picked !== 'string') return;
    try {
      // The one-time grant is consumed by the preview <img>, so submit mints
      // a fresh artwork token right before the POST.
      const token = await issueUploadToken(picked, 'artwork');
      setPreviewFailed(false);
      setArtwork({ path: picked, filename: token.filename, previewUrl: token.url });
    } catch (err) {
      toast.error(describeError(err));
    }
  }, []);

  const handleSubmit = useCallback(async () => {
    if (!audio) return;
    const uploadId = newUploadId();
    cancelRequested.current = false;
    setPhase('uploading');
    setProgress(null);
    setCancelling(false);
    setFailure(null);
    setResult(null);
    setActiveUploadId(uploadId);
    try {
      // Grants are single-use and expire after 30 minutes: mint fresh ones.
      const audioToken = await issueUploadToken(audio.path, 'audio');
      let artworkToken: string | null = null;
      if (artwork) {
        artworkToken = (await issueUploadToken(artwork.path, 'artwork')).token;
      }
      const track = await startUpload(meta, audioToken.token, artworkToken, uploadId);
      setResult(track);
      setPhase('done');
      toast.success('Uploaded');
    } catch (err) {
      if (cancelRequested.current) {
        setPhase('cancelled');
      } else {
        const message = describeError(err);
        setFailure(message);
        setPhase('error');
        toast.error(message);
      }
    } finally {
      setActiveUploadId(null);
      setCancelling(false);
    }
  }, [audio, artwork, meta]);

  const handleCancel = useCallback(() => {
    if (!activeUploadId || cancelling) return;
    cancelRequested.current = true;
    setCancelling(true);
    void cancelUpload(activeUploadId).catch(() => {});
  }, [activeUploadId, cancelling]);

  // Mirror `track:upload-progress` for the active upload only; the effect also
  // drops the subscription when the upload settles or the page unmounts.
  useEffect(() => {
    if (!activeUploadId) return;
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<{ uploadId: string; percent: number }>('track:upload-progress', (event) => {
      if (event.payload.uploadId !== activeUploadId) return;
      setProgress(Math.max(0, Math.min(100, Math.round(event.payload.percent))));
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [activeUploadId]);

  const resetForNext = useCallback(() => {
    setAudio(null);
    setArtwork(null);
    setPreviewFailed(false);
    setLargeConfirmed(false);
    setMeta(INITIAL_META);
    setPhase('idle');
    setProgress(null);
    setFailure(null);
    setResult(null);
  }, []);

  const patchMeta = useCallback((patch: Partial<UploadMeta>) => {
    setMeta((prev) => ({ ...prev, ...patch }));
  }, []);

  return (
    <div className="relative w-full min-h-screen">
      <div
        className="relative z-10 w-full max-w-[1480px] mx-auto px-4 md:px-8 pt-6 md:pt-8 pb-32"
        style={{ isolation: 'isolate' }}
      >
        <header className="mb-7">
          <h1 className="text-[24px] font-semibold tracking-tight text-white/92">{'Upload'}</h1>
          <p className="mt-1 text-[12.5px] text-white/40">
            {'Upload a track to SoundCloud with artwork and track details.'}
          </p>
        </header>

        <div className="flex w-full max-w-[720px] flex-col gap-8">
          <UploadAudio
            audio={audio}
            inspecting={inspecting}
            inspectError={inspectError}
            disabled={uploading}
            largeConfirmed={largeConfirmed}
            onPick={() => void pickAudio()}
            onConfirmLarge={() => setLargeConfirmed(true)}
          />

          {audio &&
            !blocked &&
            (uploading ? (
              <section className="flex flex-col gap-4 border-t border-white/[0.06] pt-6">
                <h2 className="text-[15px] font-semibold text-white/90">{'Uploading'}</h2>
                <UploadProgress
                  percent={progress}
                  cancelling={cancelling}
                  onCancel={handleCancel}
                />
              </section>
            ) : phase === 'done' && result ? (
              <UploadResult result={result} onReset={resetForNext} />
            ) : (
              <section className="flex flex-col gap-5 border-t border-white/[0.06] pt-6">
                <h2 className="text-[15px] font-semibold text-white/90">{'Track details'}</h2>
                {failure && (
                  <p role="alert" className="text-[12.5px] leading-snug text-red-400">
                    {failure}
                  </p>
                )}
                {phase === 'cancelled' && !failure && (
                  <p className="text-[12.5px] leading-snug text-white/55">{'Upload cancelled.'}</p>
                )}
                <UploadForm
                  value={meta}
                  onChange={patchMeta}
                  disabled={submitDisabled}
                  onSubmit={() => void handleSubmit()}
                >
                  <UploadArtwork
                    artwork={artwork}
                    previewFailed={previewFailed}
                    onPick={() => void pickArtwork()}
                    onRemove={() => setArtwork(null)}
                    onPreviewError={() => setPreviewFailed(true)}
                  />
                </UploadForm>
              </section>
            ))}
        </div>
      </div>
    </div>
  );
}
