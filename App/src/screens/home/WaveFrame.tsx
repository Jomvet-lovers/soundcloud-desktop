import type { ReactNode } from 'react';
import { ScrollView, StyleSheet, View } from 'react-native';
import { Atmosphere, PAGE_STAR_SEEDS, StarField } from '@sc/ui';
import type { Soundprint } from './useSoundprint';

/** Оболочка «Течения»: жанровая атмосфера + звёздное поле (закреплены за
 *  вьюпортом контента, не скроллятся), контент по центру max-w 1320. */
export function WaveFrame({ sound, children }: { sound: Soundprint; children: ReactNode }) {
  return (
    <View style={{ flex: 1, height: '100%' as never }}>
      <View style={StyleSheet.absoluteFill} pointerEvents="none">
        <Atmosphere tint={sound.tint} energy={sound.energy} />
      </View>
      <View style={StyleSheet.absoluteFill} pointerEvents="none">
        <StarField aura={sound.aura} seeds={PAGE_STAR_SEEDS} intensity={0.7} glow={false} />
      </View>
      <ScrollView style={{ flex: 1 }} contentContainerStyle={{ paddingBottom: 136 }}>
        <View
          style={{
            maxWidth: 1320,
            width: '100%',
            alignSelf: 'center',
            paddingHorizontal: 32,
            paddingTop: 20,
            gap: 32,
          }}
        >
          {children}
        </View>
      </ScrollView>
    </View>
  );
}
