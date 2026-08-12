import { ScrollView } from 'react-native';
import type { LyricLine } from '@sc/data';
import { ScText } from '@sc/ui';

/** Несинхронизированная лирика (донор `PlainLyrics`) — просто текст, без клика/скролла-к-строке. */
export function PlainLyrics({ lines }: { lines: LyricLine[] }) {
  const text = lines.map((l) => l.text).join('\n');
  return (
    <ScrollView
      style={{ flex: 1 }}
      showsVerticalScrollIndicator={false}
      contentContainerStyle={{ paddingHorizontal: 48, paddingVertical: 64 }}
    >
      <ScText style={{ fontSize: 22, fontWeight: '600', lineHeight: 40, letterSpacing: -0.3, color: 'rgba(255,255,255,0.7)' }}>
        {text}
      </ScText>
    </ScrollView>
  );
}
