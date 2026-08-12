import { useEffect, useRef, useState } from 'react';
import { ActivityIndicator, Animated, Pressable, View } from 'react-native';
import { type Track, useSc } from '@sc/data';
import { CheckIcon, DownloadIcon, EngagementChip, HeartIcon, LinkIcon, ListPlusIcon, MicVocalIcon, PauseIcon, PlayIcon, ScText, useScTheme } from '@sc/ui';
import { toggleLike, useLiked } from '../../player/likes';
import { lyricsUi } from '../../player/lyrics-ui';
import { playlistDialog } from '../../player/playlist-dialog';
import { formatCount, likeCountOf } from './format';

// акцент юзера (тема), не жанр — транспорт вне жанрового скоупа
function PlayPill({ isPlaying, onPress }: { isPlaying: boolean; onPress: () => void }) {
  const { accent } = useScTheme();
  const hov = useRef(new Animated.Value(0)).current;
  const sweep = useRef(new Animated.Value(0)).current;
  const [w, setW] = useState(0);
  const anim = (to: number) => Animated.spring(hov, { toValue: to, useNativeDriver: true, speed: 40, bounciness: 8 }).start();
  const onEnter = () => {
    anim(1);
    sweep.setValue(0);
    Animated.timing(sweep, { toValue: 1, duration: 650, useNativeDriver: true }).start();
  };
  return (
    <Animated.View style={{ transform: [{ scale: hov.interpolate({ inputRange: [0, 1], outputRange: [1, 1.04] }) }] }}>
      <Pressable
        onPress={onPress}
        onHoverIn={onEnter}
        onHoverOut={() => anim(0)}
        onPressIn={() => anim(0.4)}
        onPressOut={() => anim(1)}
        onLayout={(e) => setW(e.nativeEvent.layout.width)}
        style={{ overflow: 'hidden', flexDirection: 'row', alignItems: 'center', gap: 10, height: 44, paddingLeft: 16, paddingRight: 24, borderRadius: 999, backgroundColor: isPlaying ? '#fff' : accent.base, boxShadow: `0 12px 32px ${accent.glow}, inset 0 1px 0 rgba(255,255,255,0.3)` }}
      >
        {!isPlaying && w > 0 && (
          <Animated.View
            pointerEvents="none"
            // @ts-expect-error web-only backgroundImage
            style={{ position: 'absolute', top: 0, bottom: 0, width: w * 0.6, backgroundImage: 'linear-gradient(90deg, transparent, rgba(255,255,255,0.35), transparent)', transform: [{ translateX: sweep.interpolate({ inputRange: [0, 1], outputRange: [-w * 0.6, w] }) }] }}
          />
        )}
        {isPlaying ? <PauseIcon size={16} color="#000" /> : <PlayIcon size={16} color={accent.contrast} />}
        <ScText style={{ fontSize: 14, fontWeight: '700', color: isPlaying ? '#000' : accent.contrast }}>{isPlaying ? 'Пауза' : 'Слушать'}</ScText>
      </Pressable>
    </Animated.View>
  );
}

function RailBtn({ onPress, children }: { onPress: () => void; children: React.ReactNode }) {
  const a = useRef(new Animated.Value(0)).current;
  const to = (v: number) => Animated.timing(a, { toValue: v, duration: 180, useNativeDriver: true }).start();
  return (
    <Pressable onPress={onPress} onHoverIn={() => to(1)} onHoverOut={() => to(0)}>
      <Animated.View style={{ width: 40, height: 40, borderRadius: 12, alignItems: 'center', justifyContent: 'center', transform: [{ scale: a.interpolate({ inputRange: [0, 1], outputRange: [1, 1.12] }) }] }}>
        <Animated.View style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, borderRadius: 12, backgroundColor: 'rgba(255,255,255,0.08)', opacity: a }} />
        {children}
      </Animated.View>
    </Pressable>
  );
}

export function TrackActions({ track, isPlaying, onPlay }: { track: Track; isPlaying: boolean; onPlay: () => void }) {
  const sc = useSc();
  const { accent } = useScTheme();
  const liked = useLiked(track.id, track.user_favorite ?? false);
  const [copied, setCopied] = useState(false);
  const [dl, setDl] = useState<'idle' | 'busy' | 'done'>('idle');
  const idle = 'rgba(255,255,255,0.6)';

  useEffect(() => { setDl('idle'); setCopied(false); }, [track.id]);

  const download = async () => {
    if (dl !== 'idle') return;
    setDl('busy');
    try {
      const saved = await sc.tracks.download(track.id, track.artist.name, track.title, track.artwork_url);
      setDl(saved ? 'done' : 'idle');
      if (saved) setTimeout(() => setDl('idle'), 2500);
    } catch {
      setDl('idle');
    }
  };

  const likeCount = likeCountOf(track, liked);
  const showLikeCount = track.likes_count != null || liked;

  const copy = async () => {
    if (!track.permalink_url) return;
    try {
      await navigator.clipboard?.writeText(track.permalink_url);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  };

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 12, flexWrap: 'wrap' }}>
      <PlayPill isPlaying={isPlaying} onPress={onPlay} />
      <EngagementChip
        icon={<HeartIcon size={15} color={liked ? accent.base : idle} filled={liked} />}
        label={showLikeCount ? formatCount(likeCount) : ''}
        active={liked}
        accent={accent.base}
        accentGlow={accent.glow}
        accentSoft={`${accent.base}26`}
        onPress={() => toggleLike(track, sc)}
      />
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 2, height: 44, paddingHorizontal: 6, borderRadius: 16, borderWidth: 1, backgroundColor: 'rgba(255,255,255,0.04)', borderColor: 'rgba(255,255,255,0.07)' }}>
        <RailBtn onPress={() => lyricsUi.open()}>
          <MicVocalIcon size={16} color={idle} />
        </RailBtn>
        <View style={{ width: 1, height: 20, backgroundColor: 'rgba(255,255,255,0.08)', marginHorizontal: 2 }} />
        <RailBtn onPress={() => playlistDialog.open(track)}>
          <ListPlusIcon size={16} color={idle} />
        </RailBtn>
        {track.permalink_url && (
          <RailBtn onPress={copy}>
            {copied ? <CheckIcon size={16} color="#34d399" /> : <LinkIcon size={16} color={idle} />}
          </RailBtn>
        )}
        <RailBtn onPress={download}>
          {dl === 'busy' ? <ActivityIndicator size="small" color={idle} /> : dl === 'done' ? <CheckIcon size={16} color="#34d399" /> : <DownloadIcon size={16} color={idle} />}
        </RailBtn>
      </View>
    </View>
  );
}
