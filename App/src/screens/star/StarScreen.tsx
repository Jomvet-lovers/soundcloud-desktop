import { useEffect, useState } from 'react';
import { Pressable, ScrollView, StyleSheet, View } from 'react-native';
import { type Me, useSc } from '@sc/data';
import { CheckIcon, PAGE_STAR_SEEDS, ScText, StarField, StarIconOutline, useScTheme } from '@sc/ui';

const PERKS = [
  'Максимальное качество звука',
  'Без ограничений и рекламы',
  'Полный офлайн и загрузки',
  'Ранний доступ к новым фичам',
];

/** Экран STAR (премиум): звёздное поле-атмосфера (кирпич `StarField`) + статус
 *  подписки. Реальную оплату ведёт pay-бэкенд — здесь статус + перки + CTA. */
export function StarScreen({ me }: { me: Me | null }) {
  const sc = useSc();
  const { accent } = useScTheme();
  const [active, setActive] = useState<boolean | null>(null);

  useEffect(() => {
    let alive = true;
    void sc.me.subscription().then((s) => { if (alive) setActive(s); }).catch(() => { if (alive) setActive(false); });
    return () => { alive = false; };
  }, [sc]);

  return (
    <View style={{ flex: 1 }}>
      <View pointerEvents="none" style={StyleSheet.absoluteFill}>
        <StarField seeds={PAGE_STAR_SEEDS} intensity={0.85} />
      </View>

      <ScrollView contentContainerStyle={{ flexGrow: 1, alignItems: 'center', justifyContent: 'center', paddingHorizontal: 48, paddingVertical: 64, gap: 18 }}>
        <View style={{ width: 88, height: 88, borderRadius: 24, alignItems: 'center', justifyContent: 'center', backgroundColor: accent.glow, borderWidth: 1, borderColor: `${accent.base}55`, boxShadow: `0 20px 60px -12px ${accent.glow}` }}>
          <StarIconOutline size={44} color={accent.base} />
        </View>

        <ScText style={{ fontSize: 40, fontWeight: '800', letterSpacing: 2, color: 'rgba(255,255,255,0.96)' }}>STAR</ScText>
        <ScText style={{ fontSize: 15, color: 'rgba(255,255,255,0.5)', textAlign: 'center', maxWidth: 420, lineHeight: 22 }}>
          Полный доступ к звуку без компромиссов — качество, офлайн и эксклюзив.
        </ScText>

        <View
          style={{
            marginTop: 4,
            paddingHorizontal: 14,
            paddingVertical: 6,
            borderRadius: 999,
            borderWidth: 1,
            flexDirection: 'row',
            alignItems: 'center',
            gap: 6,
            borderColor: active ? `${accent.base}55` : 'rgba(255,255,255,0.12)',
            backgroundColor: active ? accent.glow : 'rgba(255,255,255,0.04)',
          }}
        >
          {active && <CheckIcon size={14} color={accent.base} />}
          <ScText style={{ fontSize: 12, fontWeight: '700', color: active ? accent.base : 'rgba(255,255,255,0.45)' }}>
            {active === null ? 'Проверяем подписку…' : active ? 'Подписка активна' : 'Подписка не активна'}
          </ScText>
        </View>

        <View style={{ marginTop: 12, gap: 10, width: '100%', maxWidth: 360 }}>
          {PERKS.map((p) => (
            <View key={p} style={{ flexDirection: 'row', alignItems: 'center', gap: 10 }}>
              <View style={{ width: 20, height: 20, borderRadius: 10, alignItems: 'center', justifyContent: 'center', backgroundColor: accent.glow }}>
                <CheckIcon size={12} color={accent.base} />
              </View>
              <ScText style={{ fontSize: 14, color: 'rgba(255,255,255,0.75)' }}>{p}</ScText>
            </View>
          ))}
        </View>

        {active === false && (
          <Pressable style={{ marginTop: 16, paddingHorizontal: 28, paddingVertical: 12, borderRadius: 999, backgroundColor: accent.base, boxShadow: `0 12px 32px -8px ${accent.glow}` }}>
            <ScText style={{ fontSize: 14, fontWeight: '800', color: accent.contrast }}>Оформить STAR</ScText>
          </Pressable>
        )}

        {me && (
          <ScText style={{ marginTop: 8, fontSize: 11, color: 'rgba(255,255,255,0.25)' }}>{me.username}</ScText>
        )}
      </ScrollView>
    </View>
  );
}
