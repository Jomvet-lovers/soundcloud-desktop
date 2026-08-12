import { memo } from 'react';
import { Pressable } from 'react-native';
import { ScText } from '@sc/ui';

interface PresetChipProps {
  label: string;
  active: boolean;
  onPress: () => void;
}

/** Чип пресета эквалайзера (донор `music/EqualizerPanel.tsx` PresetBtn). */
export const PresetChip = memo(function PresetChip({ label, active, onPress }: PresetChipProps) {
  return (
    <Pressable
      onPress={onPress}
      style={{
        paddingHorizontal: 12,
        paddingVertical: 6,
        borderRadius: 8,
        borderWidth: 1,
        backgroundColor: active ? 'rgba(255,255,255,0.1)' : 'rgba(255,255,255,0.02)',
        borderColor: active ? 'rgba(255,255,255,0.12)' : 'rgba(255,255,255,0.04)',
      }}
    >
      <ScText
        style={{ fontSize: 11, fontWeight: '600', color: active ? 'rgba(255,255,255,0.9)' : 'rgba(255,255,255,0.35)' }}
      >
        {label}
      </ScText>
    </Pressable>
  );
});
