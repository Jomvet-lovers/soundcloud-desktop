import { listen } from '@tauri-apps/api/event';
import { useCallback, useEffect, useRef, useState } from 'react';
import { toast } from 'sonner';
import { describeError } from '../../lib/diagnostics';
import {
  cancelUpload,
  inspectAudioFile,
  issueUploadToken,
  startUpload,
  type UploadedTrack,
  type UploadMeta,
} from '../../lib/upload';
import { useAuthStore } from '../../stores/auth';
import type { ArtworkSelection } from './UploadArtwork';
import type { AudioSelection } from './UploadAudio';

export type UploadPhase = 'idle' | 'uploading' | 'done' | 'error' | 'cancelled';

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

/**
 * All upload-page state and side effects (file picking, token minting, the
 * writer upload, progress subscription, cancellation) live here so the page
 * component stays a presentational composition.
 */
export function useUploadFlow() {
  const username = useAuthStore((s) => s.user?.username ?? '');
  const [audio, setAudio] = useState<AudioSelection | null>(null);
  const [inspecting, setInspecting] = useState(false);
  const [inspectError, setInspectError] = useState<string | null>(null);
  const [artwork, setArtwork] = useState<ArtworkSelection | null>(null);
  const [previewFailed, setPreviewFailed] = useState(false);
  const [meta, setMeta] = useState<UploadMeta>(INITIAL_META);
  const [phase, setPhase] = useState<UploadPhase>('idle');
  const [progress, setProgress] = useState<number | null>(null);
  const [cancelling, setCancelling] = useState(false);
  const [activeUploadId, setActiveUploadId] = useState<string | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [result, setResult] = useState<UploadedTrack | null>(null);
  const cancelRequested = useRef(false);

  const uploading = phase === 'uploading';
  const info = audio?.info ?? null;
  const blocked = (info?.too_long ?? false) || (info?.too_large ?? false);

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
    if (!audio || blocked) return;
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
  }, [audio, artwork, blocked, meta]);

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
    setMeta(INITIAL_META);
    setPhase('idle');
    setProgress(null);
    setFailure(null);
    setResult(null);
  }, []);

  const patchMeta = useCallback((patch: Partial<UploadMeta>) => {
    setMeta((prev) => ({ ...prev, ...patch }));
  }, []);

  const clearArtwork = useCallback(() => setArtwork(null), []);
  const markPreviewFailed = useCallback(() => setPreviewFailed(true), []);

  return {
    audio,
    inspecting,
    inspectError,
    artwork,
    previewFailed,
    meta,
    phase,
    progress,
    cancelling,
    failure,
    result,
    uploading,
    blocked,
    pickAudio,
    pickArtwork,
    handleSubmit,
    handleCancel,
    resetForNext,
    patchMeta,
    clearArtwork,
    markPreviewFailed,
  };
}
