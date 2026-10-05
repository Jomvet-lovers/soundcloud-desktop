import React from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { useShallow } from 'zustand/shallow';
import { art } from '../../lib/formatters';
import {
  Clock,
  Download,
  Home,
  Library,
  ListMusic,
  PanelLeftClose,
  PanelLeftOpen,
  Search,
  Settings,
} from '../../lib/icons';
import { usePerfMode } from '../../lib/perf';
import { useAppMode } from '../../stores/app-status';
import { useAuthStore } from '../../stores/auth';
import { useSettingsStore } from '../../stores/settings';
import { Avatar } from '../ui/Avatar';

type IconCmp = React.ComponentType<{ size?: number; strokeWidth?: number; className?: string }>;

const navItems: { to: string; icon: IconCmp; label: string }[] = [
  { to: '/home', icon: Home, label: 'Home' },
  { to: '/search', icon: Search, label: 'Search' },
  { to: '/library', icon: Library, label: 'Library' },
  { to: '/offline', icon: Download, label: 'Offline' },
];

const ROW = 'group relative w-full flex items-center h-10 rounded-xl transition-all duration-200';
const LABEL_T = 'max-width 320ms cubic-bezier(0.2,0.8,0.2,1), opacity 240ms ease';

// Active = flat accent wash. Readable on any accent because the accent is a
// translucent wash over the dark surface; text stays white.
const ACTIVE: React.CSSProperties = {
  color: '#fff',
  background: 'rgba(255,255,255,0.08)',
};

/** A label that always exists but folds away purely via CSS on collapse — no JS
 *  mount/unmount, so the sidebar width + labels glide together. */
function Label({
  collapsed,
  children,
  className,
}: {
  collapsed: boolean;
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <span
      className={`overflow-hidden whitespace-nowrap ${className ?? ''}`}
      style={{ maxWidth: collapsed ? 0 : '142px', opacity: collapsed ? 0 : 1, transition: LABEL_T }}
    >
      {children}
    </span>
  );
}

function IconBox({ children }: { children: React.ReactNode }) {
  return <span className="w-10 shrink-0 flex items-center justify-center">{children}</span>;
}

function NavItem({
  to,
  icon: Icon,
  label,
  collapsed,
  title,
  alert,
  active,
}: {
  to: string;
  icon: IconCmp;
  label: string;
  collapsed: boolean;
  title?: string;
  alert?: boolean;
  active?: boolean;
}) {
  return (
    <NavLink
      to={to}
      title={title}
      className={({ isActive }) => {
        const on = active ?? isActive;
        return `${ROW} ${
          on
            ? ''
            : alert
              ? 'text-white/85 bg-accent/[0.08] ring-1 ring-accent/20 hover:text-white'
              : 'text-white/45 hover:text-white/80 hover:bg-white/[0.05]'
        }`;
      }}
      style={({ isActive }) => ((active ?? isActive) ? ACTIVE : undefined)}
    >
      <IconBox>
        <Icon size={18} strokeWidth={1.9} />
      </IconBox>
      <Label collapsed={collapsed} className="text-[13px] font-medium pr-3">
        {label}
      </Label>
    </NavLink>
  );
}

export const Sidebar = React.memo(() => {
  const user = useAuthStore((s) => s.user);
  const appMode = useAppMode();
  const { collapsed, pinnedPlaylists, toggleSidebar } = useSettingsStore(
    useShallow((s) => ({
      collapsed: s.sidebarCollapsed,
      pinnedPlaylists: s.pinnedPlaylists,
      toggleSidebar: s.toggleSidebar,
    })),
  );
  const perf = usePerfMode();
  const { pathname } = useLocation();
  // The History deep page lives under /library but has its own Quick Access
  // row — keep the Library row dim there so only one item reads as active.
  const libraryActive = pathname.startsWith('/library') && !pathname.startsWith('/library/history');

  const btnCls = `${ROW} text-white/45 hover:text-white/80 hover:bg-white/[0.05] cursor-pointer`;

  return (
    <aside
      className="shrink-0 flex flex-col h-full overflow-hidden border-r border-white/[0.05] pb-3 transition-[width] duration-300 ease-[var(--ease-apple)]"
      style={{
        width: collapsed ? 56 : 196,
        transitionDuration: perf.mode === 'light' ? '0ms' : undefined,
      }}
    >
      <nav className="flex flex-col gap-0.5 px-2 pt-3">
        {navItems.map((item) => (
          <NavItem
            key={item.to}
            to={item.to}
            icon={item.icon}
            label={item.label}
            collapsed={collapsed}
            title={collapsed ? item.label : undefined}
            alert={item.to === '/offline' && appMode !== 'online'}
            active={item.to === '/library' ? libraryActive : undefined}
          />
        ))}
      </nav>

      <div className="px-2 pt-4 space-y-0.5">
        {/* Section header — folds to a hairline divider when collapsed. */}
        <div className="relative h-5 mx-1 mb-0.5">
          <span
            className="absolute inset-x-0 top-1/2 h-px"
            style={{
              background: 'rgba(255,255,255,0.07)',
              opacity: collapsed ? 1 : 0,
              transition: 'opacity 240ms ease',
            }}
          />
          <span
            className="absolute inset-0 flex items-center gap-2 px-2 text-[10px] text-white/25 font-semibold whitespace-nowrap"
            style={{ opacity: collapsed ? 0 : 1, transition: 'opacity 240ms ease' }}
          >
            {'Quick Access'}
          </span>
        </div>

        <NavItem
          to="/library/history"
          icon={Clock}
          label={'History'}
          collapsed={collapsed}
          title={collapsed ? 'History' : undefined}
        />

        {pinnedPlaylists.map((playlist) => {
          const artwork = art(playlist.artworkUrl, 'small');
          return (
            <NavLink
              key={playlist.urn}
              to={`/playlist/${encodeURIComponent(playlist.urn)}`}
              title={collapsed ? playlist.title : undefined}
              className={({ isActive }) =>
                `${ROW} ${
                  isActive ? '' : 'text-white/45 hover:text-white/80 hover:bg-white/[0.05]'
                }`
              }
              style={({ isActive }) => (isActive ? ACTIVE : undefined)}
            >
              <IconBox>
                {artwork ? (
                  <img
                    src={artwork}
                    alt=""
                    className="w-[18px] h-[18px] rounded-[5px] object-cover ring-1 ring-white/[0.1]"
                    decoding="async"
                    loading="lazy"
                  />
                ) : (
                  <ListMusic size={17} strokeWidth={1.9} />
                )}
              </IconBox>
              <Label collapsed={collapsed} className="text-[12.5px] font-medium pr-3">
                {playlist.title}
              </Label>
            </NavLink>
          );
        })}
      </div>

      <div className="flex-1" />

      <div className="px-2 pb-1 flex flex-col gap-0.5">
        <button
          type="button"
          onClick={toggleSidebar}
          title={collapsed ? 'Expand' : undefined}
          className={btnCls}
        >
          <IconBox>
            {collapsed ? (
              <PanelLeftOpen size={17} strokeWidth={1.9} />
            ) : (
              <PanelLeftClose size={17} strokeWidth={1.9} />
            )}
          </IconBox>
          <Label collapsed={collapsed} className="text-[12.5px] font-medium pr-3">
            {'Collapse'}
          </Label>
        </button>

        <NavItem
          to="/settings"
          icon={Settings}
          label={'Settings'}
          collapsed={collapsed}
          title={collapsed ? 'Settings' : undefined}
        />
      </div>

      {user && (
        <div className="px-2 pb-3">
          <NavLink
            to={`/user/${encodeURIComponent(user.urn)}`}
            title={collapsed ? user.username : undefined}
            className={({ isActive }) => `${ROW} ${isActive ? '' : 'hover:bg-white/[0.05]'}`}
            style={({ isActive }) => (isActive ? ACTIVE : undefined)}
          >
            <span className="w-10 shrink-0 flex items-center justify-center">
              <Avatar src={user.avatar_url} alt={user.username} size={26} />
            </span>
            <Label collapsed={collapsed} className="flex items-center gap-1.5 pr-3">
              <span className="text-[12.5px] text-white/55 truncate font-medium">
                {user.username}
              </span>
            </Label>
          </NavLink>
        </div>
      )}
    </aside>
  );
});
