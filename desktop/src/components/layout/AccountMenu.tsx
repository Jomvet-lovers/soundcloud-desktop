import * as Popover from '@radix-ui/react-popover';
import { useState } from 'react';
import { NavLink } from 'react-router-dom';
import { Check, Loader2, Plus } from '../../lib/icons';
import { NavSettings } from '../../lib/nav-icons';
import { type User, useAuthStore } from '../../stores/auth';
import { Avatar } from '../ui/Avatar';
import { ACTIVE, IconBox, Label, ROW } from './sidebar-primitives';

/** Account row with the profile link on the left and the quick-switch menu on
 *  right-click. Left-click navigates; the context menu is the only way to open
 *  the switcher. */
export function AccountMenu({
  user,
  collapsed,
  itemRef,
}: {
  user: User;
  collapsed: boolean;
  itemRef?: (el: HTMLAnchorElement | null) => void;
}) {
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
        <span className="block">
          <NavLink
            ref={itemRef}
            to={`/user/${encodeURIComponent(user.urn)}`}
            title={collapsed ? userName : undefined}
            aria-label={userName}
            onContextMenu={(event) => {
              event.preventDefault();
              setOpen(true);
            }}
            className={({ isActive }) => `${ROW} ${isActive ? '' : 'hover:bg-white/[0.05]'}`}
            style={({ isActive }) => (isActive ? ACTIVE : undefined)}
          >
            <IconBox>
              <Avatar
                src={user.avatar_url}
                alt={userName}
                size={18}
                shape="rounded"
                className="ring-1 ring-white/[0.1]"
              />
            </IconBox>
            <Label collapsed={collapsed} className="text-[12.5px] font-medium pr-3">
              <span className="truncate text-white/55">{userName}</span>
            </Label>
          </NavLink>
        </span>
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
