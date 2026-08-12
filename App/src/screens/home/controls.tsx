import { type ReactNode, useState } from 'react';
import { Pressable, View } from 'react-native';
import { type Track, useSc } from '@sc/data';
import { CheckIcon, ClockIcon, GlobeIcon, HeartIcon, ScText, useScTheme } from '@sc/ui';

const LANGUAGES: Array<{ code: string; name: string }> = [
  { code: 'en', name: 'English' },
  { code: 'ru', name: 'Русский' },
  { code: 'es', name: 'Español' },
  { code: 'de', name: 'Deutsch' },
  { code: 'fr', name: 'Français' },
  { code: 'it', name: 'Italiano' },
  { code: 'pt', name: 'Português' },
  { code: 'ja', name: '日本語' },
  { code: 'ko', name: '한국어' },
  { code: 'tr', name: 'Türkçe' },
  { code: 'pl', name: 'Polski' },
  { code: 'uk', name: 'Українська' },
];

/** Пилюля-тоггл пульта волны (донор hide-listened/hide-liked-toggle). */
export function HideToggle({
  icon,
  label,
  value,
  onChange,
}: {
  icon: ReactNode;
  label: string;
  value: boolean;
  onChange: (v: boolean) => void;
}) {
  const { accent } = useScTheme();
  const [hover, setHover] = useState(false);
  return (
    <Pressable
      onPress={() => onChange(!value)}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      style={({ pressed }) => ({
        flexDirection: 'row',
        alignItems: 'center',
        gap: 6,
        height: 32,
        paddingHorizontal: 12,
        borderRadius: 9999,
        borderWidth: 1,
        backgroundColor: value ? accent.glow : hover ? 'rgba(255,255,255,0.1)' : 'rgba(255,255,255,0.06)',
        borderColor: value ? accent.glow : hover ? 'rgba(255,255,255,0.14)' : 'rgba(255,255,255,0.08)',
        transform: [{ scale: pressed ? 0.96 : 1 }],
      })}
    >
      {icon}
      <ScText style={{ fontSize: 11, fontWeight: '500', color: value ? accent.base : 'rgba(255,255,255,0.7)' }}>
        {label}
      </ScText>
      <View
        style={{
          marginLeft: 4,
          width: 22,
          height: 12,
          borderRadius: 9999,
          backgroundColor: value ? accent.base : 'rgba(255,255,255,0.18)',
        }}
      >
        <View
          style={{
            position: 'absolute',
            top: 1,
            left: value ? 10 : 1,
            width: 10,
            height: 10,
            borderRadius: 5,
            backgroundColor: '#fff',
          }}
        />
      </View>
    </Pressable>
  );
}

export const HideListenedToggle = (p: { value: boolean; onChange: (v: boolean) => void }) => (
  <HideToggle icon={<ClockIcon size={12} color="rgba(255,255,255,0.7)" />} label="Свежак" {...p} />
);

export const HideLikedToggle = (p: { value: boolean; onChange: (v: boolean) => void }) => (
  <HideToggle icon={<HeartIcon size={12} color="rgba(255,255,255,0.7)" />} label="Скрыть лайки" {...p} />
);

/** Фильтр языков (донор language-filter): пилюля-триггер + выпадающий список. */
export function LanguageFilter({
  selected,
  onChange,
}: {
  selected: string[];
  onChange: (langs: string[]) => void;
}) {
  const { accent } = useScTheme();
  const [open, setOpen] = useState(false);
  const count = selected.length;
  const toggle = (code: string) =>
    onChange(selected.includes(code) ? selected.filter((c) => c !== code) : [...selected, code]);

  return (
    <View style={{ position: 'relative' }}>
      <Pressable
        onPress={() => setOpen((o) => !o)}
        style={{
          flexDirection: 'row',
          alignItems: 'center',
          gap: 6,
          height: 32,
          paddingHorizontal: 12,
          borderRadius: 9999,
          borderWidth: 1,
          backgroundColor: 'rgba(255,255,255,0.06)',
          borderColor: 'rgba(255,255,255,0.08)',
        }}
      >
        <GlobeIcon size={12} color="rgba(255,255,255,0.7)" />
        <ScText style={{ fontSize: 11, fontWeight: '500', color: 'rgba(255,255,255,0.7)' }}>
          {count === 0 ? 'Все языки' : `${count} яз`}
        </ScText>
      </Pressable>

      {open && (
        <View
          style={{
            position: 'absolute',
            bottom: 40,
            right: 0,
            width: 240,
            padding: 8,
            borderRadius: 16,
            backgroundColor: 'rgba(18,18,22,0.96)',
            borderWidth: 1,
            borderColor: 'rgba(255,255,255,0.08)',
            zIndex: 50,
          }}
        >
          {LANGUAGES.map((l) => {
            const active = selected.includes(l.code);
            return (
              <Pressable
                key={l.code}
                onPress={() => toggle(l.code)}
                style={{
                  flexDirection: 'row',
                  alignItems: 'center',
                  justifyContent: 'space-between',
                  paddingHorizontal: 10,
                  paddingVertical: 6,
                  borderRadius: 12,
                  backgroundColor: active ? accent.glow : 'transparent',
                }}
              >
                <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8 }}>
                  <ScText style={{ fontSize: 10, fontWeight: '600', textTransform: 'uppercase', color: 'rgba(255,255,255,0.4)' }}>
                    {l.code}
                  </ScText>
                  <ScText style={{ fontSize: 12, color: active ? '#fff' : 'rgba(255,255,255,0.65)' }}>
                    {l.name}
                  </ScText>
                </View>
                {active && <CheckIcon size={12} color={accent.base} />}
              </Pressable>
            );
          })}
        </View>
      )}
    </View>
  );
}

/** Лайк-кнопка трека (оптимистичный тоггл через ядро). */
export function LikeButton({ track }: { track: Track }) {
  const sc = useSc();
  const { accent } = useScTheme();
  const [liked, setLiked] = useState(!!track.user_favorite);

  const onPress = () => {
    const next = !liked;
    setLiked(next);
    const call = next ? sc.tracks.like(track.id) : sc.tracks.unlike(track.id);
    void call.catch(() => setLiked(!next));
  };

  return (
    <Pressable onPress={onPress} style={{ padding: 4 }}>
      <HeartIcon size={16} color={liked ? accent.base : 'rgba(255,255,255,0.4)'} filled={liked} />
    </Pressable>
  );
}
