import { Outlet, NavLink, useNavigate } from "react-router-dom";
import { clearAuth } from "../lib/auth";
import {
  LayoutDashboard,
  Users,
  KeyRound,
  Star,
  LogOut,
} from "lucide-react";

const navItems = [
  { to: "/", icon: LayoutDashboard, label: "Dashboard" },
  { to: "/subscriptions", icon: Users, label: "Subscriptions" },
  { to: "/oauth-apps", icon: KeyRound, label: "OAuth Apps" },
  { to: "/featured", icon: Star, label: "Featured" },
];

export default function Layout() {
  const navigate = useNavigate();

  function handleLogout() {
    clearAuth();
    navigate("/login", { replace: true });
  }

  return (
    <div className="flex min-h-screen">
      {/* Sidebar */}
      <aside className="fixed left-0 top-0 bottom-0 w-60 backdrop-blur-xl bg-white/5 border-r border-white/10 flex flex-col p-4">
        <h2 className="text-lg font-semibold text-white/90 px-3 mb-6">
          SC Admin
        </h2>

        <nav className="flex-1 space-y-1">
          {navItems.map(({ to, icon: Icon, label }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                `flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium transition-all ${
                  isActive
                    ? "bg-white/10 text-white"
                    : "text-white/50 hover:text-white/80 hover:bg-white/5"
                }`
              }
            >
              <Icon size={18} />
              {label}
            </NavLink>
          ))}
        </nav>

        <button
          onClick={handleLogout}
          className="flex items-center gap-3 px-3 py-2.5 rounded-xl text-sm font-medium text-white/50 hover:text-red-400 hover:bg-white/5 transition-all"
        >
          <LogOut size={18} />
          Logout
        </button>
      </aside>

      {/* Main content */}
      <main className="ml-60 flex-1 p-8">
        <Outlet />
      </main>
    </div>
  );
}
