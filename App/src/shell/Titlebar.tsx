import { useRef, useState } from 'react';
import { Pressable, TextInput, View } from 'react-native';
import {
  activePillGlass,
  ChevronLeftIcon,
  ChevronRightIcon,
  CloseIcon,
  GlassSurface,
  HomeIcon,
  MaximizeIcon,
  MinimizeIcon,
  searchFieldGlass,
  SearchIcon,
  useScTheme,
} from '@sc/ui';
import type { Router } from '../nav/router';

function WinButton({ onPress, danger, children }: { onPress: () => void; danger?: boolean; children: React.ReactNode }) {
  const [hover, setHover] = useState(false);
  return (
    <Pressable
      onPress={onPress}
      onPointerEnter={() => setHover(true)}
      onPointerLeave={() => setHover(false)}
      style={{
        width: 40,
        height: 36,
        borderRadius: 8,
        alignItems: 'center',
        justifyContent: 'center',
        backgroundColor: hover ? (danger ? 'rgba(239,68,68,0.8)' : 'rgba(255,255,255,0.07)') : 'transparent',
      }}
    >
      {children}
    </Pressable>
  );
}

function NavButton({ active, disabled, onPress, children }: { active?: boolean; disabled?: boolean; onPress: () => void; children: React.ReactNode }) {
  const btn = (
    <Pressable onPress={onPress} disabled={disabled} style={{ width: 32, height: 32, borderRadius: 8, alignItems: 'center', justifyContent: 'center', opacity: disabled ? 0.2 : 1 }}>
      {children}
    </Pressable>
  );
  return active ? <GlassSurface recipe={activePillGlass}>{btn}</GlassSurface> : btn;
}

/** Титлбар (донор Titlebar.tsx: высота 56px/h-14, drag-region — no-op в вебе). */
export function Titlebar({ router }: { router: Router }) {
  const { accent } = useScTheme();
  const [focused, setFocused] = useState(false);
  const inputRef = useRef<TextInput>(null);

  const toggleFullscreen = () => {
    if (document.fullscreenElement) void document.exitFullscreen();
    else void document.documentElement.requestFullscreen();
  };

  return (
    <View
      style={{
        height: 56,
        flexDirection: 'row',
        alignItems: 'center',
        gap: 12,
        paddingHorizontal: 12,
        borderBottomWidth: 0.5,
        borderBottomColor: 'rgba(255,255,255,0.07)',
      }}
    >
      <View style={{ width: 32, height: 32, borderRadius: 10, backgroundColor: accent.base }} />

      <View style={{ flexDirection: 'row', gap: 4 }}>
        <NavButton disabled={!router.canBack} onPress={router.back}>
          <ChevronLeftIcon size={17} />
        </NavButton>
        <NavButton disabled={!router.canForward} onPress={router.forward}>
          <ChevronRightIcon size={17} />
        </NavButton>
        <NavButton active={router.route.name === 'home'} onPress={() => router.navigate({ name: 'home' })}>
          <HomeIcon size={16} />
        </NavButton>
      </View>

      <View style={{ flex: 1, alignItems: 'center' }}>
        <GlassSurface
          recipe={searchFieldGlass}
          focused={focused}
          style={{ width: 600, maxWidth: '100%' as unknown as number, height: 44, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 16, gap: 10 }}
        >
          <SearchIcon size={17} color="rgba(255,255,255,0.5)" />
          <TextInput
            ref={inputRef}
            placeholder="Искать треки, артистов, плейлисты…"
            placeholderTextColor="rgba(255,255,255,0.35)"
            onFocus={() => setFocused(true)}
            onBlur={() => setFocused(false)}
            style={{ flex: 1, color: '#fff', fontSize: 13, outlineStyle: 'none' } as never}
          />
        </GlassSurface>
      </View>

      <View style={{ flexDirection: 'row', gap: 2 }}>
        <WinButton onPress={toggleFullscreen}>
          <View style={{ width: 12, height: 12, borderWidth: 1.5, borderColor: 'rgba(255,255,255,0.5)', borderRadius: 2 }} />
        </WinButton>
        <WinButton onPress={() => {}}>
          <MinimizeIcon size={13} color="rgba(255,255,255,0.5)" />
        </WinButton>
        <WinButton onPress={() => {}}>
          <MaximizeIcon size={11} color="rgba(255,255,255,0.5)" />
        </WinButton>
        <WinButton danger onPress={() => {}}>
          <CloseIcon size={14} color="rgba(255,255,255,0.5)" />
        </WinButton>
      </View>
    </View>
  );
}
