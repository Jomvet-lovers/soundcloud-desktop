// Re-export shim — keeps existing imports working
export {
  ApiError,
  apiErrorReason,
  apiRequest as api,
  fetchWithAuthFallback,
  getSessionId,
  setSessionId,
} from './api-client';
export type { ResolvedStreamingTrack } from './streaming';
export {
  buildStorageUrls,
  downloadFallbackUrls,
  resolveTrackFromStreaming,
  streamFallbackUrls,
} from './streaming';
