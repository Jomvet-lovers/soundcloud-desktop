import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { View } from 'react-native';
import { type AuthStatus, type Me, type ScClient, ScDataProvider, useSc } from '@sc/data';
import { ScThemeProvider, ScText } from '@sc/ui';
import { PlayerProvider } from './player/PlayerContext';
import { setNavHandler } from './nav/nav-bus';
import { type Route, useRouter } from './nav/router';
import { AppShell } from './shell/AppShell';
import { LoginGate } from './screens/login/LoginGate';
import { HomeScreen } from './screens/home/HomeScreen';
import { TrackScreen } from './screens/track/TrackScreen';
import { StarScreen } from './screens/star/StarScreen';
import { PlaceholderScreen } from './screens/placeholder/PlaceholderScreen';

// Пока нет экрана Настроек (акцент/perf ещё не течёт из ядра) — берём акцент
// пользователя из легаси-донора (#ff2d55). TODO: перенести в Settings-хранилище.
const ACCENT = '#ff2d55';

export function App({ client }: { client: ScClient }) {
  return (
    <ScThemeProvider accent={ACCENT}>
      <ScDataProvider client={client}>
        <Gate />
      </ScDataProvider>
    </ScThemeProvider>
  );
}

/** Логин-гейт — инвариант легаси: без сессии главный шелл не показываем. */
function Gate() {
  const sc = useSc();
  const [auth, setAuth] = useState<AuthStatus | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    sc.auth
      .status()
      .then(setAuth)
      .catch((e: Error) => setError(e.message));
  }, [sc]);

  useEffect(refresh, [refresh]);

  if (error) {
    return (
      <Center>
        <ScText level="secondary">Ядро недоступно: {error}</ScText>
      </Center>
    );
  }
  if (!auth) {
    return (
      <Center>
        <ScText level="tertiary">Подключаюсь к ядру…</ScText>
      </Center>
    );
  }
  if (!auth.authenticated) {
    return <LoginGate onAuthed={refresh} />;
  }
  return <AuthedApp />;
}

/** Каркас+роутер после логина; `me` резолвится один раз здесь и течёт вниз. */
function AuthedApp() {
  const sc = useSc();
  const router = useRouter();
  const [me, setMe] = useState<Me | null>(null);

  useEffect(() => {
    sc.me.profile().then(setMe).catch(() => setMe(null));
  }, [sc]);

  useEffect(() => {
    setNavHandler(router.navigate);
    return () => setNavHandler(null);
  }, [router.navigate]);

  return (
    <PlayerProvider>
      <AppShell router={router} me={me}>
        <Screen route={router.route} me={me} navigate={router.navigate} />
      </AppShell>
    </PlayerProvider>
  );
}

function Screen({ route, me, navigate }: { route: Route; me: Me | null; navigate: (r: Route) => void }) {
  switch (route.name) {
    case 'home':
      return <HomeScreen me={me} onOpenSearch={() => navigate({ name: 'search' })} />;
    case 'track':
      // key — чтобы локальное состояние (скачано/скопировано/композер) не протекало
      // между треками: переход на другой urn это новый экран, а не смена пропса
      return <TrackScreen key={route.urn} urn={route.urn} onBack={() => navigate({ name: 'home' })} />;
    case 'search':
      return <PlaceholderScreen title="Поиск" />;
    case 'discover':
      return <PlaceholderScreen title="Открытия" />;
    case 'library':
      return <PlaceholderScreen title="Библиотека" />;
    case 'offline':
      return <PlaceholderScreen title="Офлайн" />;
    case 'settings':
      return <PlaceholderScreen title="Настройки" />;
    case 'star':
      return <StarScreen me={me} />;
    case 'history':
      return <PlaceholderScreen title="История" />;
    default:
      return null;
  }
}

function Center({ children }: { children: ReactNode }) {
  return (
    <View style={{ flex: 1, height: '100%' as never, alignItems: 'center', justifyContent: 'center' }}>
      {children}
    </View>
  );
}
