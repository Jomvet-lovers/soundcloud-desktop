import { useSyncExternalStore } from 'react';
import type { Track } from '@sc/data';

/** Какой трек добавляем в плейлист (null — диалог закрыт). Модульный стор: любой
 *  onAddPlaylist (карточка/плеер) зовёт `open(track)`, оверлей в AppShell читает. */
let target: Track | null = null;
const listeners = new Set<() => void>();
const emit = () => {
  for (const l of listeners) l();
};

export const playlistDialog = {
  open: (track: Track) => {
    target = track;
    emit();
  },
  close: () => {
    target = null;
    emit();
  },
};

export const useAddToPlaylistTarget = (): Track | null =>
  useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => {
        listeners.delete(l);
      };
    },
    () => target,
    () => target,
  );
