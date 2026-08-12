import type { I18nKey } from './ru';

/** Английский словарь — ключи должны точно совпадать с `ru.ts` (проверяется типом). */
export const en: Record<I18nKey, string> = {
  'player.play': 'Play',
  'player.pause': 'Pause',
  'player.like': 'Like',
  'player.dislike': 'Dislike',
  'player.abLoop': 'A-B loop',
  'player.playNext': 'Play next',
  'player.addPlaylist': 'Add to playlist',
  'player.shuffle': 'Shuffle',
  'player.prev': 'Previous track',
  'player.next': 'Next track',
  'player.repeat': 'Repeat',
  'player.eq': 'Equalizer',
  'player.lyrics': 'Lyrics',
  'player.queue': 'Queue',
  'player.mute': 'Mute',
  'player.tuning': 'Speed and pitch',
  'player.openTrack': 'Open track page',
};
