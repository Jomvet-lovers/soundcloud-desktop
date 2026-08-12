import { useEffect, useRef, useState } from 'react';
import { Animated, Pressable, View } from 'react-native';
import Svg, { Path } from 'react-native-svg';
import type { Track } from '@sc/data';
import { CloseIcon, Cover, EqBars, formatDuration, GlassSurface, queuePanelGlass, ScText, useScTheme, VirtualList } from '@sc/ui';
import { usePlayerState } from '../player/PlayerContext';

const PANEL_WIDTH = 360;

function trackWord(n: number): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod100 >= 11 && mod100 <= 14) return 'треков';
  if (mod10 === 1) return 'трек';
  if (mod10 >= 2 && mod10 <= 4) return 'трека';
  return 'треков';
}

function Chevron({ dir, size = 9, color }: { dir: 'up' | 'down'; size?: number; color: string }) {
  const d = dir === 'up' ? 'M4 10l4-4 4 4' : 'M4 6l4 4 4-4';
  return (
    <Svg width={size} height={size} viewBox="0 0 16 16" fill="none">
      <Path d={d} stroke={color} strokeWidth={1.6} strokeLinecap="round" strokeLinejoin="round" />
    </Svg>
  );
}

function QueueHeader({ count, onClose }: { count: number; onClose: () => void }) {
  return (
    <View
      style={{
        flexDirection: 'row',
        alignItems: 'center',
        justifyContent: 'space-between',
        paddingHorizontal: 18,
        paddingTop: 18,
        paddingBottom: 12,
      }}
    >
      <View style={{ flexDirection: 'row', alignItems: 'baseline', gap: 8 }}>
        <ScText style={{ fontSize: 17, fontWeight: '700' }}>Очередь</ScText>
        {count > 0 && (
          <ScText token="counter" level="tertiary" style={{ fontSize: 11 }}>
            {count} {trackWord(count)}
          </ScText>
        )}
      </View>
      <Pressable
        onPress={onClose}
        style={{ width: 28, height: 28, borderRadius: 8, alignItems: 'center', justifyContent: 'center' }}
      >
        <CloseIcon size={16} color="rgba(255,255,255,0.4)" />
      </Pressable>
    </View>
  );
}

function QueueRow({
  track,
  active,
  playing,
  onPress,
  onRemove,
  onMoveUp,
  onMoveDown,
  canMoveUp,
  canMoveDown,
}: {
  track: Track;
  active: boolean;
  playing: boolean;
  onPress: () => void;
  onRemove?: () => void;
  onMoveUp?: () => void;
  onMoveDown?: () => void;
  canMoveUp?: boolean;
  canMoveDown?: boolean;
}) {
  const { accent } = useScTheme();
  const [hover, setHover] = useState(false);
  const reorderable = !active && (onMoveUp || onMoveDown);

  return (
    <Pressable
      onPress={onPress}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      style={{
        position: 'relative',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 10,
        borderRadius: 12,
        paddingVertical: 7,
        paddingLeft: 12,
        paddingRight: 10,
        backgroundColor: active ? accent.glow : hover ? 'rgba(255,255,255,0.035)' : 'transparent',
      }}
    >
      {active && (
        <View
          style={{ position: 'absolute', left: 0, top: 8, bottom: 8, width: 2.5, borderRadius: 2, backgroundColor: accent.base }}
        />
      )}

      <Cover url={track.artwork_url} size={40} radius={8} artSize="t200x200" />

      <View style={{ flex: 1, minWidth: 0 }}>
        <ScText numberOfLines={1} style={{ fontSize: 13.5, fontWeight: '500', color: active ? accent.hover : 'rgba(255,255,255,0.88)' }}>
          {track.title}
        </ScText>
        <ScText numberOfLines={1} style={{ marginTop: 1, fontSize: 12, color: 'rgba(255,255,255,0.4)' }}>
          {track.artist.name}
        </ScText>
      </View>

      {active ? (
        <EqBars active={playing} height={12} />
      ) : (
        <>
          <ScText token="counter" level="tertiary" style={{ fontSize: 11.5, width: 34, textAlign: 'right' }}>
            {formatDuration(track.duration_ms)}
          </ScText>

          {hover && reorderable && (
            <View style={{ flexDirection: 'row', alignItems: 'center' }}>
              <Pressable
                disabled={!canMoveUp}
                hitSlop={4}
                onPress={(e) => {
                  e.stopPropagation();
                  onMoveUp?.();
                }}
                style={{ width: 18, height: 18, alignItems: 'center', justifyContent: 'center', opacity: canMoveUp ? 1 : 0.25 }}
              >
                <Chevron dir="up" color="rgba(255,255,255,0.55)" />
              </Pressable>
              <Pressable
                disabled={!canMoveDown}
                hitSlop={4}
                onPress={(e) => {
                  e.stopPropagation();
                  onMoveDown?.();
                }}
                style={{ width: 18, height: 18, alignItems: 'center', justifyContent: 'center', opacity: canMoveDown ? 1 : 0.25 }}
              >
                <Chevron dir="down" color="rgba(255,255,255,0.55)" />
              </Pressable>
            </View>
          )}

          {hover && onRemove && (
            <Pressable
              hitSlop={4}
              onPress={(e) => {
                e.stopPropagation();
                onRemove();
              }}
              style={{ width: 20, height: 20, borderRadius: 6, alignItems: 'center', justifyContent: 'center' }}
            >
              <CloseIcon size={12} color="rgba(255,255,255,0.45)" />
            </Pressable>
          )}
        </>
      )}
    </Pressable>
  );
}

/** Правый дровер очереди (донор `QueuePanel`) — текущий трек сверху, «Далее» ниже
 *  со скроллом; клик прыгает по очереди, ховер открывает reorder/remove. */
export function QueuePanel({ open, onClose }: { open: boolean; onClose: () => void }) {
  const { queue, index, currentTrack, playing, togglePlayPause, playQueue, reorderQueue, removeFromQueue } = usePlayerState();
  const translateX = useRef(new Animated.Value(PANEL_WIDTH)).current;

  useEffect(() => {
    Animated.timing(translateX, { toValue: 0, duration: 260, useNativeDriver: true }).start();
  }, [translateX]);

  if (!open) return null;

  const upcoming = queue.slice(index + 1);

  return (
    <>
      <Pressable
        onPress={onClose}
        style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, backgroundColor: 'rgba(0,0,0,0.35)', zIndex: 40 }}
      />
      <Animated.View
        style={{ position: 'absolute', top: 0, right: 0, bottom: 0, width: PANEL_WIDTH, zIndex: 50, transform: [{ translateX }] }}
      >
        <GlassSurface
          recipe={queuePanelGlass}
          style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, borderLeftWidth: 0.5, borderLeftColor: 'rgba(255,255,255,0.06)' }}
        />

        <View style={{ flex: 1 }}>
          <QueueHeader count={queue.length} onClose={onClose} />

          {currentTrack && (
            <View style={{ paddingHorizontal: 12, paddingBottom: 6 }}>
              <ScText token="label" level="tertiary" style={{ paddingHorizontal: 6, marginBottom: 6 }}>
                Сейчас играет
              </ScText>
              <QueueRow track={currentTrack} active playing={playing} onPress={togglePlayPause} />
            </View>
          )}

          <VirtualList
            data={upcoming}
            itemHeight={56}
            style={{ flex: 1 }}
            contentContainerStyle={{ paddingHorizontal: 12, paddingBottom: 16 }}
            keyExtractor={(track, localIdx) => `${track.id}-${index + 1 + localIdx}`}
            header={
              upcoming.length > 0 ? (
                <ScText token="label" level="tertiary" style={{ paddingHorizontal: 6, marginTop: 4, marginBottom: 6 }}>
                  Далее · {upcoming.length}
                </ScText>
              ) : null
            }
            empty={
              queue.length === 0 ? (
                <View style={{ alignItems: 'center', paddingTop: 60 }}>
                  <ScText level="secondary" style={{ fontSize: 13 }}>
                    Очередь пуста
                  </ScText>
                </View>
              ) : null
            }
            renderItem={(track, localIdx) => {
              const absIdx = index + 1 + localIdx;
              return (
                <QueueRow
                  track={track}
                  active={false}
                  playing={false}
                  onPress={() => playQueue(queue, absIdx)}
                  onRemove={() => removeFromQueue(track.id)}
                  onMoveUp={() => reorderQueue(absIdx, absIdx - 1)}
                  onMoveDown={() => reorderQueue(absIdx, absIdx + 1)}
                  canMoveUp={localIdx > 0}
                  canMoveDown={localIdx < upcoming.length - 1}
                />
              );
            }}
          />
        </View>
      </Animated.View>
    </>
  );
}
