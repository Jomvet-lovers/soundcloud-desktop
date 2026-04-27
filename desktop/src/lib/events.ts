import { useAuthStore } from '../stores/auth';
import { api } from './api';

export type SoundWaveEvent =
  | 'like'
  | 'local_like'
  | 'playlist_add'
  | 'full_play'
  | 'skip'
  | 'dislike';

/** Fire-and-forget event recorder for SoundWave taste model. */
export function recordEvent(eventType: SoundWaveEvent, scTrackId: string): void {
  if (!scTrackId) return;
  const scUserId = useAuthStore.getState().user?.urn;
  if (!scUserId) return;

  api('/events', {
    method: 'POST',
    body: JSON.stringify({ scUserId, scTrackId, eventType }),
  }).catch(() => {});
}
