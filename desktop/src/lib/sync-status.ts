import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';

interface SyncErrorPayload {
  method?: string;
  url?: string;
  status?: number;
  captcha?: boolean;
  error?: string;
}

listen<SyncErrorPayload>('direct:sync-error', (event) => {
  const { captcha, status } = event.payload ?? {};
  const message = captcha
    ? "SoundCloud's bot protection blocked this change — it's saved locally. Try again in a moment."
    : status === 401
      ? 'SoundCloud session expired — this change is saved locally but not synced. Sign in again to resume syncing.'
      : "Couldn't sync this change to SoundCloud — it's saved on this device only.";
  toast.error(message, { id: 'direct-sync-error' });
});
