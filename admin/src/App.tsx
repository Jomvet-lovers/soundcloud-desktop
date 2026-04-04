import { Routes, Route, Navigate } from "react-router-dom";
import { getAuth } from "./lib/auth";
import Layout from "./components/Layout";
import Login from "./pages/Login";
import Dashboard from "./pages/Dashboard";
import Subscriptions from "./pages/Subscriptions";
import OAuthApps from "./pages/OAuthApps";
import Featured from "./pages/Featured";

function ProtectedRoute({ children }: { children: React.ReactNode }) {
  const auth = getAuth();
  if (!auth) return <Navigate to="/login" replace />;
  return <>{children}</>;
}

export default function App() {
  return (
    <Routes>
      <Route path="/login" element={<Login />} />
      <Route
        path="/*"
        element={
          <ProtectedRoute>
            <Layout />
          </ProtectedRoute>
        }
      >
        <Route index element={<Dashboard />} />
        <Route path="subscriptions" element={<Subscriptions />} />
        <Route path="oauth-apps" element={<OAuthApps />} />
        <Route path="featured" element={<Featured />} />
      </Route>
    </Routes>
  );
}
