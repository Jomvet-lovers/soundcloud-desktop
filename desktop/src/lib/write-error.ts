import { toast } from 'sonner';
import { ApiError, apiErrorReason } from './api';

/**
 * Report a write SoundCloud refused.
 *
 * Writes are applied locally only after SoundCloud accepts them, so by the time
 * this runs nothing was changed. A 502 carries the backend's reason; a 401 is
 * left to the auth-recovery flow, which owns the re-login prompt.
 */
export function toastWriteError(error: unknown, fallback: string): void {
  if (error instanceof ApiError && error.status === 401) return;
  toast.error(apiErrorReason(error) ?? fallback);
}
