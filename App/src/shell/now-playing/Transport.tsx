import { useEffect, useRef, useState } from 'react';
import { Animated, Easing, Pressable, View } from 'react-native';
import {
  AbLoopIcon,
  GlassSurface,
  PauseIcon,
  PlayIcon,
  playOrb,
  RepeatIcon,
  ShuffleIcon,
  SkipBackIcon,
  SkipForwardIcon,
  Tooltip,
  useScTheme,
} from '@sc/ui';
import { useT } from '../../i18n';
import type { AbLoop, RepeatMode } from '../../player/PlayerContext';
import { IconButton } from './IconButton';

/** Play-орб — герой пилюли (донор `.npb-play`): ховер scale 1.06, active 0.95,
 *  пока играет — пульсирующее кольцо-эхо (`::after` донора). */
function PlayOrb({
  playing,
  disabled,
  tooltip,
  onPress,
}: {
  playing: boolean;
  disabled: boolean;
  tooltip?: string;
  onPress: () => void;
}) {
  const { accent } = useScTheme();
  const [hovered, setHovered] = useState(false);
  const [pressed, setPressed] = useState(false);
  const hover = useRef(new Animated.Value(0)).current;
  const pulse = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    Animated.timing(hover, { toValue: hovered ? 1 : 0, duration: 250, useNativeDriver: true }).start();
  }, [hovered, hover]);

  useEffect(() => {
    if (!playing) return;
    pulse.setValue(0);
    const loop = Animated.loop(
      Animated.timing(pulse, { toValue: 1, duration: 2800, easing: Easing.out(Easing.cubic), useNativeDriver: true }),
    );
    loop.start();
    return () => loop.stop();
  }, [playing, pulse]);

  const hoverScale = hover.interpolate({ inputRange: [0, 1], outputRange: [1, 1.06] });
  const hoverLift = hover.interpolate({ inputRange: [0, 1], outputRange: [0, -1] });

  return (
    <Tooltip label={tooltip}>
      <Pressable
        onPress={onPress}
        disabled={disabled}
        onHoverIn={() => setHovered(true)}
        onHoverOut={() => setHovered(false)}
        onPressIn={() => setPressed(true)}
        onPressOut={() => setPressed(false)}
        style={{ width: 48, height: 48, marginHorizontal: 3 }}
      >
        <Animated.View
          style={{
            width: 48,
            height: 48,
            transform: [{ translateY: hoverLift }, { scale: pressed ? 0.95 : hoverScale }],
          }}
        >
          <GlassSurface recipe={playOrb} style={{ width: 48, height: 48, alignItems: 'center', justifyContent: 'center' }}>
            {playing ? <PauseIcon size={20} color={accent.contrast} /> : <PlayIcon size={20} color={accent.contrast} />}
          </GlassSurface>
          <Animated.View
            pointerEvents="none"
            style={{
              position: 'absolute',
              inset: 0,
              borderRadius: 24,
              borderWidth: 1.5,
              borderColor: accent.base,
              opacity: playing ? pulse.interpolate({ inputRange: [0, 0.7, 1], outputRange: [0.7, 0, 0] }) : 0,
              transform: [{ scale: playing ? pulse.interpolate({ inputRange: [0, 0.7, 1], outputRange: [1, 1.4, 1.4] }) : 1 }],
            }}
          />
        </Animated.View>
      </Pressable>
    </Tooltip>
  );
}

/** A-B-повтор (донор `AbLoopBtn`): тап циклит A→B→сброс от текущей позиции;
 *  активна — акцентная заливка, ждём B — пульсирующая точка справа-сверху. */
function AbLoopBtn({ abLoop, tooltip, onCycle }: { abLoop: AbLoop | null; tooltip?: string; onCycle: () => void }) {
  const { accent } = useScTheme();
  const active = abLoop != null;
  const awaitingB = abLoop != null && abLoop.b == null;
  const pulse = useRef(new Animated.Value(1)).current;

  useEffect(() => {
    if (!awaitingB) return;
    const loop = Animated.loop(
      Animated.sequence([
        Animated.timing(pulse, { toValue: 0.3, duration: 550, useNativeDriver: true }),
        Animated.timing(pulse, { toValue: 1, duration: 550, useNativeDriver: true }),
      ]),
    );
    loop.start();
    return () => loop.stop();
  }, [awaitingB, pulse]);

  return (
    <Tooltip label={tooltip}>
      <Pressable
        onPress={onCycle}
        style={{
          width: 30, height: 30, borderRadius: 15, alignItems: 'center', justifyContent: 'center',
          backgroundColor: active ? accent.glow : 'transparent',
        }}
      >
        <AbLoopIcon size={16} color={active ? accent.base : 'rgba(255,255,255,0.55)'} />
        {awaitingB && (
          <Animated.View
            style={{ position: 'absolute', right: 3, top: 3, width: 6, height: 6, borderRadius: 3, backgroundColor: accent.base, opacity: pulse, boxShadow: `0 0 6px ${accent.glow}` }}
          />
        )}
      </Pressable>
    </Tooltip>
  );
}

export function Transport({
  playing,
  disabled,
  shuffle,
  repeat,
  abLoop,
  onTogglePlay,
  onPrev,
  onNext,
  onToggleShuffle,
  onCycleRepeat,
  onCycleAb,
}: {
  playing: boolean;
  disabled: boolean;
  shuffle: boolean;
  repeat: RepeatMode;
  abLoop: AbLoop | null;
  onTogglePlay: () => void;
  onPrev: () => void;
  onNext: () => void;
  onToggleShuffle: () => void;
  onCycleRepeat: () => void;
  onCycleAb: () => void;
}) {
  const { accent } = useScTheme();
  const idle = 'rgba(255,255,255,0.55)';
  const t = useT();

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 2 }}>
      <IconButton size={30} onPress={onToggleShuffle} tooltip={t('player.shuffle')}>
        <ShuffleIcon size={16} color={shuffle ? accent.base : idle} active={shuffle} />
      </IconButton>
      <IconButton size={36} onPress={onPrev} tooltip={t('player.prev')}>
        <SkipBackIcon size={20} color={idle} />
      </IconButton>
      <PlayOrb
        playing={playing}
        disabled={disabled}
        tooltip={playing ? t('player.pause') : t('player.play')}
        onPress={onTogglePlay}
      />
      <IconButton size={36} onPress={onNext} tooltip={t('player.next')}>
        <SkipForwardIcon size={20} color={idle} />
      </IconButton>
      <IconButton size={30} onPress={onCycleRepeat} tooltip={t('player.repeat')}>
        <RepeatIcon size={16} color={repeat !== 'off' ? accent.base : idle} mode={repeat} />
      </IconButton>
      <AbLoopBtn abLoop={abLoop} tooltip={t('player.abLoop')} onCycle={onCycleAb} />
    </View>
  );
}
