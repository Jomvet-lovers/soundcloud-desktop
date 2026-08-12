import { useSyncExternalStore } from 'react';
import type { ScClient, Track } from '@sc/data';

/**
 * Общий стор лайков — единый источник истины на весь фронт (home, в будущем library
 * и пр.). Лайк/анлайк в любом месте обновляет ВСЕ подписанные кнопки + список для
 * «очереди по лайкам». Живёт вне React (useSyncExternalStore), как position/download.
 */
let liked = new Set<string>();
let unliked = new Set<string>(); // явные анлайки (перебивают fallback `user_favorite`)
let list: Track[] = []; // лайкнутые треки, recent-first (кэш — стабильная ссылка между изменениями)
let version = 0;
const listeners = new Set<() => void>();
const unlikeListeners = new Set<(id: string) => void>();

const emit = () => {
  version++;
  for (const l of listeners) l();
};

/** Наполнить стор загруженными лайками (library.likedTracks). Recent-first. */
export function seedLikes(tracks: Track[]): void {
  liked = new Set(tracks.map((t) => t.id));
  unliked = new Set();
  list = tracks;
  emit();
}

function apply(track: Track, isLiked: boolean): void {
  if (isLiked) {
    liked.add(track.id);
    unliked.delete(track.id);
    list = [track, ...list.filter((t) => t.id !== track.id)];
  } else {
    liked.delete(track.id);
    unliked.add(track.id);
    list = list.filter((t) => t.id !== track.id);
  }
  emit();
  if (!isLiked) for (const l of unlikeListeners) l(track.id);
}

/** Текущий статус с учётом fallback на `track.user_favorite` (не-хук). */
export const isLikedNow = (id: string, fallback: boolean): boolean =>
  unliked.has(id) ? false : liked.has(id) || fallback;
const effective = isLikedNow;

/** Оптимистичный тогл + RPC + откат при ошибке. */
export function toggleLike(track: Track, sc: ScClient): void {
  const next = !effective(track.id, track.user_favorite ?? false);
  apply(track, next);
  void (next ? sc.tracks.like(track.id) : sc.tracks.unlike(track.id)).catch(() => apply(track, !next));
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};

/** Реактивный статус лайка одного трека. `fallback` — `track.user_favorite`. */
export const useLiked = (id: string, fallback = false): boolean =>
  useSyncExternalStore(
    subscribe,
    () => effective(id, fallback),
    () => effective(id, fallback),
  );

/** Подписка на любые изменения лайков (для списков-полок: ре-рендер + чтение
 *  статусов через `isLikedNow`, чтобы не звать хук на каждый трек в маппере). */
export const useLikesVersion = (): number =>
  useSyncExternalStore(subscribe, () => version, () => version);

/** Список лайкнутых (для «очереди по лайкам»/полки «Любимые треки»). */
export const useLikedTracks = (): Track[] => useSyncExternalStore(subscribe, () => list, () => list);
export const getLikedTracks = (): Track[] => list;

/** Подписка «трек разлайкан» — плеер убирает его из likes-очереди. */
export function onUnlike(cb: (id: string) => void): () => void {
  unlikeListeners.add(cb);
  return () => {
    unlikeListeners.delete(cb);
  };
}
