import React, { useCallback, useEffect, useRef, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { useShallow } from 'zustand/shallow';
import { PanelLeftClose, PanelLeftOpen } from '../../lib/icons';
import {
  NavFeed,
  NavHistory,
  NavHome,
  NavLibrary,
  NavOffline,
  NavSearch,
  NavSettings,
  NavUpload,
} from '../../lib/nav-icons';
import { usePerfMode } from '../../lib/perf';
import { useAppMode } from '../../stores/app-status';
import { useAuthStore } from '../../stores/auth';
import { claimLegacyPins, usePinnedPlaylists } from '../../stores/pinned-playlists';
import { useSettingsStore } from '../../stores/settings';
import { AccountMenu } from './AccountMenu';
import { SidebarPins } from './SidebarPins';
import { ACTIVE, IconBox, Label, ROW } from './sidebar-primitives';

type IconCmp = React.ComponentType<{ size?: number; strokeWidth?: number; className?: string }>;

const navItems: { to: string; icon: IconCmp; label: string }[] = [
  { to: '/home', icon: NavHome, label: 'Home' },
  { to: '/feed', icon: NavFeed, label: 'Feed' },
  { to: '/search', icon: NavSearch, label: 'Search' },
  { to: '/library', icon: NavLibrary, label: 'Library' },
  { to: '/library/history', icon: NavHistory, label: 'History' },
  { to: '/offline', icon: NavOffline, label: 'Offline' },
  { to: '/upload', icon: NavUpload, label: 'Upload' },
];

// Shared row styles live in `sidebar-primitives.tsx`.

function NavItem({
  to,
  icon: Icon,
  label,
  collapsed,
  title,
  alert,
  active,
  itemRef,
}: {
  to: string;
  icon: IconCmp;
  label: string;
  collapsed: boolean;
  title?: string;
  alert?: boolean;
  active?: boolean;
  itemRef?: (el: HTMLAnchorElement | null) => void;
}) {
  return (
    <NavLink
      ref={itemRef}
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
  const { collapsed, toggleSidebar } = useSettingsStore(
    useShallow((s) => ({
      collapsed: s.sidebarCollapsed,
      toggleSidebar: s.toggleSidebar,
    })),
  );
  const pinnedPlaylists = usePinnedPlaylists();
  const perf = usePerfMode();
  const { pathname } = useLocation();
  // The History deep page lives under /library but has its own nav row 遯ｶ繝ｻ  // keep the Library row dim there so only one item reads as active.
  const libraryActive = pathname.startsWith('/library') && !pathname.startsWith('/library/history');

  // Shared active-row indicator: one wash that slides between rows. Rows are
  // all h-10, so only transform moves (no width/height animation).
  const asideRef = useRef<HTMLElement>(null);
  const itemRefs = useRef(new Map<string, HTMLElement>());
  const [indicator, setIndicator] = useState({ top: 0, visible: false, instant: true });

  const registerItem = useCallback(
    (key: string) => (el: HTMLElement | null) => {
      if (el) itemRefs.current.set(key, el);
      else itemRefs.current.delete(key);
    },
    [],
  );

  const activeKey = (() => {
    if (pathname.startsWith('/library/history')) return '/library/history';
    if (libraryActive) return '/library';
    const pin = pinnedPlaylists.find((p) => pathname === `/playlist/${encodeURIComponent(p.urn)}`);
    if (pin) return `pin:${pin.urn}`;
    if (pathname.startsWith('/settings')) return '/settings';
    if (user && pathname === `/user/${encodeURIComponent(user.urn)}`) return 'me';
    return navItems.find((i) => pathname === i.to || pathname.startsWith(`${i.to}/`))?.to ?? null;
  })();

  // biome-ignore lint/correctness/useExhaustiveDependencies: re-measure when the pinned list or sidebar width changes (the active element itself does not move).
  useEffect(() => {
    const aside = asideRef.current;
    const el = activeKey ? itemRefs.current.get(activeKey) : null;
    if (!aside || !el) {
      setIndicator((v) => (v.visible ? { ...v, visible: false } : v));
      return;
    }
    const top = el.getBoundingClientRect().top - aside.getBoundingClientRect().top;
    setIndicator((v) => ({ top, visible: true, instant: v.instant }));
    const raf = requestAnimationFrame(() =>
      setIndicator((v) => (v.instant ? { ...v, instant: false } : v)),
    );
    return () => cancelAnimationFrame(raf);
  }, [activeKey, pinnedPlaylists, collapsed, pathname]);

  // One-shot migration: the old shared pin list is claimed by the first
  // account the shell resolves (see `claimLegacyPins`).
  // biome-ignore lint/correctness/useExhaustiveDependencies: rerun when the signed-in account resolves.
  useEffect(() => {
    claimLegacyPins();
  }, [user?.id]);

  const btnCls = `${ROW} text-white/45 hover:text-white/80 hover:bg-white/[0.05] cursor-pointer`;

  return (
    <aside
      ref={asideRef}
      className="relative shrink-0 flex flex-col h-full overflow-hidden border-r border-white/[0.05] pb-3 transition-[width] duration-300 ease-[var(--ease-apple)]"
      style={{
        width: collapsed ? 56 : 196,
        transitionDuration: perf.mode === 'light' ? '0ms' : undefined,
      }}
    >
      {/* Shared active-row wash 遯ｶ繝ｻslides between rows on navigation. */}
      <span
        aria-hidden
        className={`sidebar-indicator absolute inset-x-2 top-0 h-10 rounded-xl pointer-events-none ${
          indicator.instant ? 'sidebar-indicator--instant' : ''
        }`}
        style={{
          transform: `translateY(${indicator.top}px)`,
          opacity: indicator.visible ? 1 : 0,
          background: 'rgba(255,255,255,0.08)',
        }}
      />
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
            itemRef={registerItem(item.to)}
          />
        ))}
      </nav>

      <SidebarPins
        pinnedPlaylists={pinnedPlaylists}
        collapsed={collapsed}
        registerItem={registerItem}
      />

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
          icon={NavSettings}
          label={'Settings'}
          collapsed={collapsed}
          title={collapsed ? 'Settings' : undefined}
          itemRef={registerItem('/settings')}
        />
      </div>

      {user && (
        <div className="px-2 pb-3">
          <AccountMenu user={user} collapsed={collapsed} itemRef={registerItem('me')} />
        </div>
      )}
    </aside>
  );
});
