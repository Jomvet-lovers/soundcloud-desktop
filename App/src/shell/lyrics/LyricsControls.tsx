import { useCallback, useEffect, useRef, useState } from 'react';
import { Animated, Pressable, View } from 'react-native';
import { type Track, useSc } from '@sc/data';
import {
  ExternalLinkIcon,
  HeartIcon,
  ListPlusIcon,
  PauseIcon,
  PlayIcon,
  RepeatIcon,
  ShuffleIcon,
  SkipBackIcon,
  SkipForwardIcon,
  ThumbsDownIcon,
  Tooltip,
  useScTheme,
} from '@sc/ui';
import { useT } from '../../i18n';
import { go } from '../../nav/nav-bus';
import { toggleLike, useLiked } from '../../player/likes';
import { lyricsUi } from '../../player/lyrics-ui';
import { playlistDialog } from '../../player/playlist-dialog';
import { scId, type PlayerState } from '../../player/PlayerContext';
import { IconButton } from '../now-playing/IconButton';

/** Белый play-орб (донор `.w-14 h-14 bg-white`) — герой ряда в оверлее лирики. */
function WhitePlay({ playing, onPress, tooltip }: { playing: boolean; onPress: () => void; tooltip: string }) {
  const [hovered, setHovered] = useState(false);
  const [pressed, setPressed] = useState(false);
  const s = useRef(new Animated.Value(0)).current;
  useEffect(() => {
    Animated.spring(s, { toValue: pressed ? -1 : hovered ? 1 : 0, useNativeDriver: true, speed: 40, bounciness: 6 }).start();
  }, [hovered, pressed, s]);
  return (
    <Tooltip label={tooltip}>
      <Pressable
        onPress={onPress}
        onHoverIn={() => setHovered(true)}
        onHoverOut={() => setHovered(false)}
        onPressIn={() => setPressed(true)}
        onPressOut={() => setPressed(false)}
        style={{ marginHorizontal: 4 }}
      >
        <Animated.View
          style={{
            width: 56,
            height: 56,
            borderRadius: 28,
            backgroundColor: '#fff',
            alignItems: 'center',
            justifyContent: 'center',
            boxShadow: '0 8px 24px rgba(0,0,0,0.35)',
            transform: [{ scale: s.interpolate({ inputRange: [-1, 0, 1], outputRange: [0.95, 1, 1.05] }) }],
          }}
        >
          {playing ? <PauseIcon size={22} color="#0a0a0c" /> : <PlayIcon size={22} color="#0a0a0c" />}
        </Animated.View>
      </Pressable>
    </Tooltip>
  );
}

/** Ряд управления в оверлее лирики (донор `LyricsControls.Controls`, Image #4):
 *  плейлист · лайк · шафл · prev · БЕЛЫЙ play · next · repeat · дизлайк.
 *  Бар под оверлеем скрыт — это единственное управление на экране. */
export function LyricsControls({ track, player }: { track: Track; player: PlayerState }) {
  const sc = useSc();
  const { accent } = useScTheme();
  const t = useT();
  const idle = 'rgba(255,255,255,0.35)';
  const liked = useLiked(track.id, track.user_favorite ?? false);
  const [disliked, setDisliked] = useState(false);

  useEffect(() => {
    let alive = true;
    setDisliked(false);
    void sc.tracks.dislikeStatus(scId(track)).then((d) => { if (alive) setDisliked(d); }).catch(() => {});
    return () => { alive = false; };
  }, [track, sc]);

  const onDislike = useCallback(() => {
    const next = !disliked;
    setDisliked(next);
    if (next && liked) toggleLike(track, sc);
    void (next ? sc.tracks.dislike(scId(track)) : sc.tracks.undislike(scId(track))).catch(() => setDisliked(!next));
    if (next && player.currentTrack?.id === track.id) player.next();
  }, [disliked, liked, track, sc, player]);

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 4 }}>
      <IconButton size={36} onPress={() => playlistDialog.open(track)} tooltip={t('player.addPlaylist')}>
        <ListPlusIcon size={20} color={idle} />
      </IconButton>
      <IconButton size={44} onPress={() => toggleLike(track, sc)} tooltip={t('player.like')}>
        <HeartIcon size={20} color={liked ? accent.base : idle} filled={liked} />
      </IconButton>
      <IconButton size={36} onPress={player.toggleShuffle} tooltip={t('player.shuffle')}>
        <ShuffleIcon size={16} color={player.shuffle ? accent.base : idle} active={player.shuffle} />
      </IconButton>
      <IconButton size={40} onPress={player.prev} tooltip={t('player.prev')}>
        <SkipBackIcon size={20} color="rgba(255,255,255,0.6)" />
      </IconButton>
      <WhitePlay playing={player.playing} onPress={player.togglePlayPause} tooltip={player.playing ? t('player.pause') : t('player.play')} />
      <IconButton size={40} onPress={player.next} tooltip={t('player.next')}>
        <SkipForwardIcon size={20} color="rgba(255,255,255,0.6)" />
      </IconButton>
      <IconButton size={36} onPress={player.cycleRepeat} tooltip={t('player.repeat')}>
        <RepeatIcon size={16} color={player.repeat !== 'off' ? accent.base : idle} mode={player.repeat} />
      </IconButton>
      <IconButton size={44} onPress={onDislike} tooltip={t('player.dislike')}>
        <ThumbsDownIcon size={18} color={disliked ? '#fb7185' : idle} filled={disliked} />
      </IconButton>
      <IconButton size={40} onPress={() => { lyricsUi.close(); go({ name: 'track', urn: track.id }); }} tooltip={t('player.openTrack')}>
        <ExternalLinkIcon size={18} color={idle} />
      </IconButton>
    </View>
  );
}
