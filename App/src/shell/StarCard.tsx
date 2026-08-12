import { useEffect, useState } from 'react';
import { Pressable, View } from 'react-native';
import { useSc } from '@sc/data';
import { GlassSurface, ScText, StarIconOutline } from '@sc/ui';

const starGlass = {
  radius: 14,
  blur: 16,
  saturate: 1,
  fill: [{ kind: 'solid' as const, color: 'rgba(139, 92, 246, 0.16)' }],
  border: { width: 0.5, color: 'rgba(168, 85, 247, 0.35)' },
};

/** Промо-карточка подписки (донор StarSubscription StarCard) — реальный статус из sc.me.subscription(). */
export function StarCard({ collapsed, onPress }: { collapsed: boolean; onPress: () => void }) {
  const sc = useSc();
  const [active, setActive] = useState<boolean | null>(null);

  useEffect(() => {
    sc.me.subscription().then(setActive).catch(() => setActive(null));
  }, [sc]);

  if (active === null) return null;

  return (
    <Pressable onPress={onPress}>
      <GlassSurface recipe={starGlass} style={{ padding: 10, flexDirection: 'row', alignItems: 'center', gap: 8 }}>
        <StarIconOutline size={15} color="#facc15" />
        {!collapsed && (
          <View style={{ flex: 1, minWidth: 0 }}>
            <ScText numberOfLines={1} style={{ fontSize: 12, fontWeight: '600' }}>
              Подписка Star
            </ScText>
            <ScText numberOfLines={1} level="secondary" style={{ fontSize: 10.5 }}>
              {active ? 'Активна' : 'Узнать больше'}
            </ScText>
          </View>
        )}
      </GlassSurface>
    </Pressable>
  );
}
