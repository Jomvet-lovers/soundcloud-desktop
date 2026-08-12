import { View } from 'react-native';
import { ScText } from '@sc/ui';

/** Экран ещё не собран (см. трекинг задач Ф1: Search/Discover/Library и т.д.). */
export function PlaceholderScreen({ title }: { title: string }) {
  return (
    <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', gap: 8 }}>
      <ScText token="heroTitle" style={{ fontSize: 24 }}>
        {title}
      </ScText>
      <ScText level="tertiary">Экран в работе</ScText>
    </View>
  );
}
