import { Pressable, View } from 'react-native';
import { AudioLinesIcon, ChevronLeftIcon, ChevronRightIcon, CloseIcon, ScText, useScTheme } from '@sc/ui';
import type { LyricsTab } from '../../player/lyrics-ui';

export const HEADER_HEIGHT = 64;

const glassBtn = {
  width: 36,
  height: 36,
  borderRadius: 18,
  alignItems: 'center' as const,
  justifyContent: 'center' as const,
  backgroundColor: 'rgba(0,0,0,0.35)',
  borderWidth: 1,
  borderColor: 'rgba(255,255,255,0.08)',
};

function TabButton({
  active,
  disabled,
  onPress,
  children,
}: {
  active: boolean;
  disabled?: boolean;
  onPress: () => void;
  children: string;
}) {
  return (
    <Pressable
      onPress={disabled ? undefined : onPress}
      disabled={disabled}
      style={{
        paddingHorizontal: 16,
        paddingVertical: 7,
        borderRadius: 999,
        backgroundColor: active ? 'rgba(255,255,255,0.14)' : 'transparent',
        opacity: disabled ? 0.35 : 1,
      }}
    >
      <ScText style={{ fontSize: 12.5, fontWeight: '600', color: active ? '#ffffff' : 'rgba(255,255,255,0.4)' }}>
        {children}
      </ScText>
    </Pressable>
  );
}

/** Плавающий верхний хедер (донор `LyricsHeader`): табы по центру в своей стеклянной
 *  пилюле, close + переключатель сплита — фиксированным правым кластером (не съезжают
 *  между фокус/сплит режимами). */
export function LyricsHeader({
  tab,
  rightPanelOpen,
  visualizer,
  onSelectTab,
  onTogglePanel,
  onToggleVisualizer,
  onClose,
}: {
  tab: LyricsTab;
  rightPanelOpen: boolean;
  visualizer: boolean;
  onSelectTab: (tab: LyricsTab) => void;
  onTogglePanel: () => void;
  onToggleVisualizer: () => void;
  onClose: () => void;
}) {
  const { accent } = useScTheme();
  return (
    <View
      pointerEvents="box-none"
      style={{
        position: 'absolute',
        top: 0,
        left: 0,
        right: 0,
        height: HEADER_HEIGHT,
        zIndex: 20,
        flexDirection: 'row',
        alignItems: 'center',
        paddingHorizontal: 20,
      }}
    >
      <View style={{ flex: 1 }} />

      <View
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 2,
          padding: 4,
          borderRadius: 999,
          borderWidth: 1,
          borderColor: 'rgba(255,255,255,0.08)',
          backgroundColor: 'rgba(0,0,0,0.35)',
        }}
      >
        <TabButton active={rightPanelOpen && tab === 'lyrics'} onPress={() => onSelectTab('lyrics')}>
          Лирика
        </TabButton>
        <TabButton active={rightPanelOpen && tab === 'comments'} onPress={() => onSelectTab('comments')}>
          Комментарии
        </TabButton>
      </View>

      <View style={{ flex: 1, flexDirection: 'row', justifyContent: 'flex-end', alignItems: 'center', gap: 8 }}>
        <Pressable
          onPress={onToggleVisualizer}
          style={[glassBtn, visualizer && { backgroundColor: accent.glow, borderColor: accent.base }]}
          accessibilityLabel="Визуализатор"
        >
          <AudioLinesIcon size={16} color={visualizer ? accent.base : 'rgba(255,255,255,0.6)'} />
        </Pressable>
        <Pressable onPress={onTogglePanel} style={glassBtn} accessibilityLabel="Свернуть/развернуть панель">
          {rightPanelOpen ? (
            <ChevronRightIcon size={16} color="rgba(255,255,255,0.6)" />
          ) : (
            <ChevronLeftIcon size={16} color="rgba(255,255,255,0.6)" />
          )}
        </Pressable>
        <Pressable onPress={onClose} style={glassBtn} accessibilityLabel="Закрыть">
          <CloseIcon size={18} color="rgba(255,255,255,0.7)" />
        </Pressable>
      </View>
    </View>
  );
}
