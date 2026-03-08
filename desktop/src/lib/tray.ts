import { listen } from '@tauri-apps/api/event';
import { usePlayerStore } from '../stores/player';
import { handlePrev } from './audio';

listen<string>('tray-action', (event) => {
  const store = usePlayerStore.getState();
  switch (event.payload) {
    case 'play_pause':
      store.togglePlay();
      break;
    case 'next':
      store.next();
      break;
    case 'prev':
      handlePrev();
      break;
  }
});
