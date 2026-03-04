import { useEffect, useState } from "react";
import { BrowserRouter, Routes, Route } from "react-router-dom";
import { AppShell } from "./components/layout/AppShell";
import { Home } from "./pages/Home";
import { Search } from "./pages/Search";
import { Library } from "./pages/Library";
import { TrackPage } from "./pages/TrackPage";
import { PlaylistPage } from "./pages/PlaylistPage";
import { UserPage } from "./pages/UserPage";
import { Login } from "./pages/Login";
import { useAuthStore } from "./stores/auth";

export default function App() {
  const { isAuthenticated, sessionId, fetchUser } = useAuthStore();
  const [checking, setChecking] = useState(true);

  useEffect(() => {
    if (sessionId) {
      fetchUser()
        .catch(() => useAuthStore.getState().logout())
        .finally(() => setChecking(false));
    } else {
      setChecking(false);
    }
  }, []);

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
    <BrowserRouter>
      <Routes>
        <Route element={<AppShell />}>
          <Route index element={<Home />} />
          <Route path="search" element={<Search />} />
          <Route path="library" element={<Library />} />
          <Route path="track/:urn" element={<TrackPage />} />
          <Route path="playlist/:urn" element={<PlaylistPage />} />
          <Route path="user/:urn" element={<UserPage />} />
        </Route>
      </Routes>
    </BrowserRouter>
  );
}
