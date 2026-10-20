import * as Popover from '@radix-ui/react-popover';
import React, { useCallback, useEffect, useRef, useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import { useShallow } from 'zustand/shallow';
import { art } from '../../lib/formatters';
import { Check, ListMusic, Loader2, PanelLeftClose, PanelLeftOpen, Plus } from '../../lib/icons';
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
import { type User, useAuthStore } from '../../stores/auth';
import { useSettingsStore } from '../../stores/settings';
import { playlistMenuHandler } from '../../stores/track-menu';
import { Avatar } from '../ui/Avatar';

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

const ROW = 'group relative w-full flex items-center h-10 rounded-xl transition-all duration-200';
const LABEL_T = 'max-width 320ms cubic-bezier(0.2,0.8,0.2,1), opacity 240ms ease';

// Active = white text; the shared sliding indicator supplies the wash.
const ACTIVE: React.CSSProperties = {
  color: '#fff',
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

/** Account avatar with a right-click quick-switch menu. Left-click keeps the
 *  profile navigation; the context menu is the only way to open the switcher. */
function AccountMenu({ user }: { user: User }) {
  const accounts = useAuthStore((s) => s.accounts);
  const activeId = useAuthStore((s) => s.activeId);
  const switchAccount = useAuthStore((s) => s.switchAccount);
  const addAccount = useAuthStore((s) => s.addAccount);
  const [open, setOpen] = useState(false);
  const [pending, setPending] = useState<number | 'add' | null>(null);
  const [error, setError] = useState<string | null>(null);
  const userName = user.username.trim() || `Account ${user.id}`;

  const active = accounts.find((account) => account.id === activeId) ?? null;
  const ordered = active
    ? [active, ...accounts.filter((account) => account.id !== active.id)]
    : accounts;

  const handleOpenChange = (next: boolean) => {
    setOpen(next);
    if (!next) setError(null);
  };

  const handleSwitch = async (id: number) => {
    if (pending !== null) return;
    setError(null);
    setPending(id);
    try {
      await switchAccount(id);
      setOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(null);
    }
  };

  const handleAdd = async () => {
    if (pending !== null) return;
    setError(null);
    setPending('add');
    try {
      await addAccount();
      setOpen(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setPending(null);
    }
  };

  return (
    <Popover.Root open={open} onOpenChange={handleOpenChange}>
      <Popover.Anchor asChild>
        <NavLink
          to={`/user/${encodeURIComponent(user.urn)}`}
          title={userName}
          aria-label={userName}
          onContextMenu={(event) => {
            event.preventDefault();
            setOpen(true);
          }}
          className={({ isActive }) =>
            `mx-auto flex h-10 w-10 items-center justify-center rounded-xl transition-colors ${
              isActive ? 'bg-white/[0.08]' : 'hover:bg-white/[0.05]'
            }`
          }
        >
          <Avatar
            src={user.avatar_url}
            alt={userName}
            size={22}
            shape="rounded"
            className="ring-1 ring-white/[0.1]"
          />
        </NavLink>
      </Popover.Anchor>
      <Popover.Portal>
        <Popover.Content
          side="right"
          align="center"
          sideOffset={8}
          collisionPadding={12}
          aria-label={'Accounts'}
          className="z-[200] max-h-[420px] w-[252px] overflow-y-auto rounded-xl border border-white/[0.1] bg-[#141417] p-2 outline-none"
        >
          <p className="px-2 pb-1 pt-0.5 text-[11.5px] font-medium text-white/45">{'Accounts'}</p>

          <div className="flex flex-col">
            {ordered.map((account) => {
              const label = account.username.trim() || `Account ${account.id}`;
              if (account.id === activeId) {
                return (
                  <div key={account.id} className="flex items-center gap-2 rounded-md px-2 py-2">
                    <Avatar src={account.avatar_url} alt={label} size={22} shape="rounded" />
                    <span className="min-w-0 flex-1 truncate text-[12.5px] font-medium text-white/90">
                      {label}
                    </span>
                    <Check size={14} className="shrink-0 text-accent" />
                    <span className="sr-only">{'Active account'}</span>
                  </div>
                );
              }
              return (
                <button
                  key={account.id}
                  type="button"
                  disabled={pending !== null}
                  onClick={() => void handleSwitch(account.id)}
                  className="flex cursor-pointer items-center gap-2 rounded-md px-2 py-2 text-left text-white/60 transition-colors hover:bg-white/[0.06] hover:text-white/90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Avatar src={account.avatar_url} alt={label} size={22} shape="rounded" />
                  <span className="min-w-0 flex-1 truncate text-[12.5px]">{label}</span>
                  {pending === account.id && (
                    <Loader2 size={14} className="shrink-0 animate-spin text-white/50" />
                  )}
                </button>
              );
            })}
          </div>

          <div className="my-1 border-t border-white/[0.07]" />

          <button
            type="button"
            disabled={pending !== null}
            onClick={() => void handleAdd()}
            className="flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-2 text-left text-[12.5px] text-white/60 transition-colors hover:bg-white/[0.06] hover:text-white/90 disabled:cursor-not-allowed disabled:opacity-50"
          >
            {pending === 'add' ? (
              <Loader2 size={14} className="shrink-0 animate-spin" />
            ) : (
              <Plus size={14} className="shrink-0" />
            )}
            <span className="min-w-0 flex-1 truncate">
              {pending === 'add' ? 'Waiting for sign-in...' : 'Add account'}
            </span>
          </button>

          <NavLink
            to="/settings"
            onClick={() => setOpen(false)}
            className={({ isActive }) =>
              `flex items-center gap-2 rounded-md px-2 py-2 text-[12.5px] transition-colors ${
                isActive ? 'text-white' : 'text-white/60 hover:bg-white/[0.06] hover:text-white/90'
              }`
            }
          >
            <NavSettings size={14} className="shrink-0" />
            <span className="min-w-0 flex-1 truncate">{'Settings'}</span>
          </NavLink>

          {error && (
            <p role="alert" className="px-2 pb-1 pt-1.5 text-[11px] leading-snug text-red-400">
              {error}
            </p>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
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
  // The History deep page lives under /library but has its own nav row —
  // keep the Library row dim there so only one item reads as active.
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
      {/* Shared active-row wash — slides between rows on navigation. */}
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

      {pinnedPlaylists.length > 0 && (
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

          {pinnedPlaylists.map((playlist) => {
            const artwork = art(playlist.artworkUrl, 'small');
            return (
              <NavLink
                key={playlist.urn}
                ref={registerItem(`pin:${playlist.urn}`)}
                to={`/playlist/${encodeURIComponent(playlist.urn)}`}
                title={collapsed ? playlist.title : undefined}
                onContextMenu={playlistMenuHandler({ urn: playlist.urn })}
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
      )}

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
          <AccountMenu user={user} />
        </div>
      )}
    </aside>
  );
});
