import { useState } from 'react';
import { Pressable, View } from 'react-native';
import type { Me } from '@sc/data';
import {
  activePillGlass,
  ChevronLeftIcon,
  ChevronRightIcon,
  ClockIcon,
  CompassIcon,
  Cover,
  DownloadIcon,
  GlassSurface,
  HomeIcon,
  LibraryIcon,
  ScText,
  SearchIcon,
  SettingsIcon,
  StarIconOutline,
  useScTheme,
} from '@sc/ui';
import type { Route } from '../nav/router';
import { StarCard } from './StarCard';

const NAV: { route: Route; label: string; Icon: typeof HomeIcon; accentIcon?: boolean }[] = [
  { route: { name: 'home' }, label: 'Главная', Icon: HomeIcon },
  { route: { name: 'search' }, label: 'Поиск', Icon: SearchIcon },
  { route: { name: 'discover' }, label: 'Каталог', Icon: CompassIcon },
  { route: { name: 'library' }, label: 'Библиотека', Icon: LibraryIcon },
  { route: { name: 'star' }, label: 'STAR', Icon: StarIconOutline, accentIcon: true },
  { route: { name: 'offline' }, label: 'Оффлайн', Icon: DownloadIcon },
];

const EXPANDED = 196;
const COLLAPSED = 56;
const ROW_H = 40;

function Row({
  active,
  onPress,
  icon,
  label,
  collapsed,
}: {
  active: boolean;
  onPress: () => void;
  icon: React.ReactNode;
  label: string | null;
  collapsed: boolean;
}) {
  const item = (
    <Pressable
      onPress={onPress}
      style={{ height: ROW_H, flexDirection: 'row', alignItems: 'center', gap: 10, paddingHorizontal: 10, borderRadius: 12 }}
    >
      <View style={{ width: 20, alignItems: 'center' }}>{icon}</View>
      {!collapsed && label && (
        <ScText numberOfLines={1} style={{ fontSize: 13, fontWeight: active ? '600' : '400' }}>
          {label}
        </ScText>
      )}
    </Pressable>
  );
  return active ? <GlassSurface recipe={activePillGlass}>{item}</GlassSurface> : item;
}

export function Sidebar({ route, navigate, me }: { route: Route; navigate: (r: Route) => void; me: Me | null }) {
  const { accent } = useScTheme();
  const [collapsed, setCollapsed] = useState(false);
  const width = collapsed ? COLLAPSED : EXPANDED;

  return (
    <View
      style={{
        width,
        borderRightWidth: 0.5,
        borderRightColor: 'rgba(255,255,255,0.05)',
        paddingTop: 16,
        paddingBottom: 12,
        paddingHorizontal: 8,
        justifyContent: 'space-between',
      }}
    >
      <View>
        <View style={{ gap: 2 }}>
          {NAV.map(({ route: r, label, Icon, accentIcon }) => {
            const active = r.name === route.name;
            const color = active ? accent.base : accentIcon ? 'rgba(168,85,247,0.9)' : 'rgba(255,255,255,0.45)';
            return (
              <Row
                key={r.name}
                active={active}
                collapsed={collapsed}
                onPress={() => navigate(r)}
                icon={<Icon size={17} color={color} />}
                label={label}
              />
            );
          })}
        </View>

        {!collapsed && (
          <ScText token="label" level="tertiary" style={{ fontSize: 10, marginTop: 20, marginBottom: 4, paddingHorizontal: 10 }}>
            быстрый доступ
          </ScText>
        )}
        <Row
          active={route.name === 'history'}
          collapsed={collapsed}
          onPress={() => navigate({ name: 'history' })}
          icon={<ClockIcon size={16} color={route.name === 'history' ? accent.base : 'rgba(255,255,255,0.45)'} />}
          label="История"
        />
      </View>

      <View style={{ gap: 8 }}>
        <StarCard collapsed={collapsed} onPress={() => navigate({ name: 'star' })} />

        <View style={{ gap: 2 }}>
          <Pressable onPress={() => setCollapsed((c) => !c)} style={{ height: ROW_H, alignItems: 'center', justifyContent: 'center' }}>
            {collapsed ? <ChevronRightIcon size={14} color="rgba(255,255,255,0.35)" /> : <ChevronLeftIcon size={14} color="rgba(255,255,255,0.35)" />}
          </Pressable>

          <Row
            active={route.name === 'settings'}
            collapsed={collapsed}
            onPress={() => navigate({ name: 'settings' })}
            icon={<SettingsIcon size={16} color={route.name === 'settings' ? accent.base : 'rgba(255,255,255,0.45)'} />}
            label="Настройки"
          />

          {me && (
            <View style={{ height: ROW_H, flexDirection: 'row', alignItems: 'center', gap: 10, paddingHorizontal: 10 }}>
              <Cover url={me.avatar_url} size={26} radius={13} artSize="t67x67" />
              {!collapsed && (
                <View style={{ flexDirection: 'row', alignItems: 'center', gap: 6, minWidth: 0 }}>
                  <ScText numberOfLines={1} level="secondary" style={{ fontSize: 12.5 }}>
                    {me.username}
                  </ScText>
                  {me.premium && <StarIconOutline size={11} color="#facc15" />}
                </View>
              )}
            </View>
          )}
        </View>
      </View>
    </View>
  );
}
