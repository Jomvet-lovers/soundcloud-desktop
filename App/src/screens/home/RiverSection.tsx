import type { ReactNode } from 'react';
import { View } from 'react-native';
import { ScText } from '@sc/ui';

export type RiverTone = 'open' | 'panel' | 'deep';

/** Рамка участка реки: заголовок + «почему это здесь». Узлы/ветки рисует
 *  RiverBraid по якорю-обёртке — здесь только контент. */
export function RiverSection({
  title,
  why,
  tone = 'open',
  children,
}: {
  title: string;
  why: string;
  tone?: RiverTone;
  children: ReactNode;
}) {
  const head = (
    <View style={{ marginBottom: 16 }}>
      <ScText style={{ fontSize: 21, fontWeight: '700', lineHeight: 24, letterSpacing: -0.32, color: 'rgba(255,255,255,0.92)' }}>
        {title}
      </ScText>
      <ScText style={{ marginTop: 4, fontSize: 13, lineHeight: 18, color: 'rgba(255,255,255,0.5)' }}>
        {why}
      </ScText>
    </View>
  );

  if (tone === 'open') {
    return (
      <View style={{ minWidth: 0 }}>
        {head}
        {children}
      </View>
    );
  }

  return (
    <View style={{ minWidth: 0 }}>
      {head}
      <View
        style={{
          borderRadius: 16,
          borderWidth: 1,
          padding: 16,
          borderColor: tone === 'deep' ? 'rgba(255,255,255,0.05)' : 'rgba(255,255,255,0.07)',
          backgroundColor: tone === 'deep' ? 'rgba(5,5,8,0.55)' : 'rgba(255,255,255,0.025)',
        }}
      >
        {children}
      </View>
    </View>
  );
}
