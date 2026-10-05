import { listen } from '@tauri-apps/api/event';
import { toast } from 'sonner';
import i18n from '../i18n';

interface SyncErrorPayload {
  method?: string;
  url?: string;
  status?: number;
  captcha?: boolean;
  error?: string;
}

listen<SyncErrorPayload>('direct:sync-error', (event) => {
  const { captcha, status } = event.payload ?? {};
  const key = captcha ? 'sync.captchaFailed' : status === 401 ? 'sync.tokenExpired' : 'sync.failed';
  toast.error(i18n.t(key), { id: 'direct-sync-error' });
});
