import { type Track, useSc } from '@sc/data';
import type { TrackCardProps } from '@sc/ui';
import { useT } from '../../i18n';
import { go } from '../../nav/nav-bus';
import { isLikedNow, toggleLike, useLikesVersion } from '../../player/likes';
import { playlistDialog } from '../../player/playlist-dialog';
import type { PlayerState } from '../../player/PlayerContext';

/** Track → бейдж-поля для `TrackCard`/строк. */
export const badgeOf = (t: Track): TrackCardProps['badge'] =>
  t.badge ? { storageState: t.badge.storage_state, indexState: t.badge.index_state, storageQuality: t.badge.storage_quality } : undefined;

// общий источник пропсов TrackCard для Home-полок и «Похожего» — карточки одинаковые
export function useCardProps(player: PlayerState) {
  const sc = useSc();
  const t = useT();
  useLikesVersion(); // подписка: полка ре-рендерится на любой лайк/анлайк
  return (track: Track, queue: Track[]): TrackCardProps => {
    const current = player.currentTrack?.id === track.id;
    return {
      title: track.title,
      artist: track.artist.name,
      artworkUrl: track.artwork_url,
      durationMs: track.duration_ms,
      playCount: track.play_count,
      badge: badgeOf(track),
      current,
      playing: current && player.playing,
      liked: isLikedNow(track.id, track.user_favorite ?? false),
      onPlay: () => player.toggle(track, queue),
      onPause: () => player.toggle(track),
      onOpen: () => go({ name: 'track', urn: track.id }),
      onAddQueue: () => player.playNext(track),
      onAddPlaylist: () => playlistDialog.open(track),
      onToggleLike: () => toggleLike(track, sc),
      likeLabel: t('player.like'),
      addQueueLabel: t('player.playNext'),
      addPlaylistLabel: t('player.addPlaylist'),
    };
  };
}
