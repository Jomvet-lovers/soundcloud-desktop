import { trackedInvoke as invoke } from './diagnostics';

/** Result of `upload_inspect`: file facts plus the pre-upload verdict. */
export interface AudioInspection {
  size: number;
  filename: string;
  duration_ms: number | null;
  extension_ok: boolean;
  /** Over the 500 MB request-body limit: continue only after a UI confirm. */
  too_large: boolean;
  /** Over 24 hours: the upload is refused. */
  too_long: boolean;
  warnings: string[];
}

/** `track_upload` metadata. Snake_case keys match the Rust struct. */
export interface UploadMeta {
  title: string;
  artist?: string;
  genre?: string;
  tag_list?: string;
  description?: string;
  sharing?: 'public' | 'private';
  embeddable_by?: 'all' | 'me' | 'none';
  permalink?: string;
  release_date?: string;
  downloadable?: boolean;
  commentable?: boolean;
}

/** Track created by a successful upload. */
export interface UploadedTrack {
  id: number;
  urn: string;
  permalink_url: string;
  title: string;
  state: string;
}

/** One-time file grant returned by `upload_issue_token`. */
export interface UploadToken {
  token: string;
  url: string;
  filename: string;
  content_type: string;
  size: number;
}

/** Check an audio file (extension, size, duration) before uploading. */
export async function inspectAudioFile(path: string): Promise<AudioInspection> {
  return invoke<AudioInspection>('upload_inspect', { path });
}

/** Mint a one-time grant for the audio or artwork file. Fresh grants are
 *  required on every upload attempt; they expire after 30 minutes. */
export async function issueUploadToken(
  path: string,
  kind: 'audio' | 'artwork',
): Promise<UploadToken> {
  return invoke<UploadToken>('upload_issue_token', { path, kind });
}

/** POST the track through the writer WebView. `uploadId` is echoed in
 *  `track:upload-progress` and accepted by `cancelUpload`. */
export async function startUpload(
  meta: UploadMeta,
  audioToken: string,
  artworkToken: string | null,
  uploadId: string,
): Promise<UploadedTrack> {
  return invoke<UploadedTrack>('track_upload', {
    meta,
    audioToken,
    artworkToken,
    uploadId,
  });
}

/** Abort the in-flight XHR for `uploadId`. Unknown ids are a no-op. */
export async function cancelUpload(uploadId: string): Promise<void> {
  return invoke<void>('upload_cancel', { uploadId });
}
