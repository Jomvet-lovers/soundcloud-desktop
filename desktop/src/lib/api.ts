// Re-export shim — keeps existing imports working
export {
  ApiError,
  apiRequest as api,
  fetchWithAuthFallback,
  getSessionId,
  setSessionId,
} from './api-client';
export type { ResolvedStreamingTrack } from './streaming';
export { buildStorageUrls, resolveTrackFromStreaming, streamFallbackUrls } from './streaming';
