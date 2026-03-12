import { useEffect, useState } from 'react';
import { BrowserRouter, Route, Routes } from 'react-router-dom';
import { Toaster } from 'sonner';
import { useShallow } from 'zustand/shallow';
import { AppShell } from './components/layout/AppShell';
import { ThemeProvider } from './components/ThemeProvider';
import { UpdateChecker } from './components/UpdateChecker';
import { Home } from './pages/Home';
import { Library } from './pages/Library';
import { Login } from './pages/Login';
import { PlaylistPage } from './pages/PlaylistPage';
import { Search } from './pages/Search';
import { Settings } from './pages/Settings';
import { TrackPage } from './pages/TrackPage';
import { UserPage } from './pages/UserPage';
import { useAuthStore } from './stores/auth';

export default function App() {
  const { isAuthenticated, sessionId, fetchUser } = useAuthStore(
    useShallow((s) => ({
      isAuthenticated: s.isAuthenticated,
      sessionId: s.sessionId,
      fetchUser: s.fetchUser,
    })),
  );
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    if (sessionId) {
      fetchUser()
        .catch(() => useAuthStore.getState().logout())
        .finally(() => setChecking(false));
    } else {
      setChecking(false);
    }
  }, [fetchUser, sessionId]);

  if (checking) {
    return (
      <div className="h-screen flex items-center justify-center">
        <div className="w-5 h-5 border-2 border-accent border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  if (!isAuthenticated) {
    return <Login />;
  }

  return (
    <ThemeProvider>
      <BrowserRouter>
        <Toaster
          theme="dark"
          position="bottom-right"
          toastOptions={{
            style: {
              background: 'rgba(30, 30, 34, 0.9)',
              backdropFilter: 'blur(20px)',
              border: '1px solid rgba(255,255,255,0.08)',
              color: 'rgba(255,255,255,0.85)',
              fontSize: '13px',
            },
          }}
        />
        <UpdateChecker />
        <Routes>
          <Route element={<AppShell />}>
            <Route index element={<Home />} />
            <Route path="search" element={<Search />} />
            <Route path="library" element={<Library />} />
            <Route path="track/:urn" element={<TrackPage />} />
            <Route path="playlist/:urn" element={<PlaylistPage />} />
            <Route path="user/:urn" element={<UserPage />} />
            <Route path="settings" element={<Settings />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </ThemeProvider>
  );
}
