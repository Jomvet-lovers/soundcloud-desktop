import { useEffect, useRef, useState } from 'react';
import { Animated, Pressable } from 'react-native';
import { Tooltip } from '@sc/ui';

/** Круглая кнопка транспорта (донор `.npb-play` соседи: shuffle/prev/next/repeat/eq/…).
 *  Ховер — лёгкий подъём + подсветка фона, active — сжатие, оба через Animated.
 *  Опц. `tooltip` — подсказка над кнопкой (см. `@sc/ui` Tooltip). */
export function IconButton({
  size = 30,
  tooltip,
  onPress,
  children,
}: {
  size?: number;
  tooltip?: string;
  onPress?: () => void;
  children: React.ReactNode;
}) {
  const [hovered, setHovered] = useState(false);
  const [pressed, setPressed] = useState(false);
  const hover = useRef(new Animated.Value(0)).current;

  useEffect(() => {
    Animated.timing(hover, { toValue: hovered ? 1 : 0, duration: 200, useNativeDriver: false }).start();
  }, [hovered, hover]);

  return (
    <Tooltip label={tooltip}>
      <Pressable
        onPress={onPress}
        onHoverIn={() => setHovered(true)}
        onHoverOut={() => setHovered(false)}
        onPressIn={() => setPressed(true)}
        onPressOut={() => setPressed(false)}
        style={{ width: size, height: size, borderRadius: size / 2, alignItems: 'center', justifyContent: 'center' }}
      >
        <Animated.View
          style={{
            width: '100%',
            height: '100%',
            borderRadius: size / 2,
            alignItems: 'center',
            justifyContent: 'center',
            backgroundColor: hover.interpolate({
              inputRange: [0, 1],
              outputRange: ['rgba(255,255,255,0)', 'rgba(255,255,255,0.08)'],
            }),
            transform: [
              { translateY: hover.interpolate({ inputRange: [0, 1], outputRange: [0, -1] }) },
              { scale: pressed ? 0.9 : 1 },
            ],
          }}
        >
          {children}
        </Animated.View>
      </Pressable>
    </Tooltip>
  );
}
