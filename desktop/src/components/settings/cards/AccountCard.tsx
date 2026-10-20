import { useNavigate } from 'react-router-dom';
import { Check, Loader2, Plus, Trash2, User } from '../../../lib/icons';
import { useAuthStore } from '../../../stores/auth';
import { accountName, orderedAccounts } from '../../account/account-utils';
import { useAccountActions } from '../../account/useAccountActions';
import { Avatar } from '../../ui/Avatar';
import { Card, Divider, Row } from '../primitives';

export function AccountCard() {
  const navigate = useNavigate();
  const accounts = useAuthStore((s) => s.accounts);
  const activeId = useAuthStore((s) => s.activeId);
  const { pending, error, busy, switchTo, remove, add, signOut } = useAccountActions();

  const active = accounts.find((account) => account.id === activeId) ?? null;
  const ordered = orderedAccounts(accounts, activeId);

  return (
    <Card title={'Account'} icon={<User size={17} />}>
      <div className="flex flex-col gap-4">
        {active && (
          <div className="flex items-center gap-3">
            <Avatar src={active.avatar_url} alt={accountName(active)} size={36} shape="rounded" />
            <div className="min-w-0">
              <p className="truncate text-[13.5px] font-medium text-white/90">
                {accountName(active)}
              </p>
              <p className="mt-0.5 text-[11.5px] text-white/45">
                {active.premium ? 'SoundCloud Go+' : 'Signed in'}
              </p>
            </div>
          </div>
        )}

        {accounts.length > 0 && (
          <button
            type="button"
            onClick={() => navigate('/upload')}
            className="flex w-fit cursor-pointer items-center gap-2 rounded-md border border-white/[0.08] bg-white/[0.04] px-4 py-2 text-[12.5px] font-medium text-white/75 transition-colors hover:bg-white/[0.08] hover:text-white/90"
          >
            {'Upload a track'}
          </button>
        )}

        {accounts.length > 0 && (
          <>
            <Divider />
            <div className="flex flex-col">
              <p className="mb-1 text-[11.5px] font-medium text-white/45">{'Signed-in accounts'}</p>
              <div className="flex flex-col divide-y divide-white/[0.05]">
                {ordered.map((account) => {
                  const isActive = account.id === activeId;
                  const label = accountName(account);
                  return (
                    <Row
                      key={account.id}
                      title={
                        <>
                          <Avatar src={account.avatar_url} alt={label} size={22} shape="rounded" />
                          <span className={`min-w-0 truncate ${isActive ? 'text-white/90' : ''}`}>
                            {label}
                          </span>
                        </>
                      }
                    >
                      {isActive ? (
                        <>
                          <Check size={15} className="text-accent" />
                          <span className="sr-only">{'Active account'}</span>
                        </>
                      ) : (
                        <>
                          <button
                            type="button"
                            disabled={busy}
                            onClick={() => void switchTo(account.id)}
                            className="flex min-w-[52px] cursor-pointer items-center justify-center rounded-md px-2 py-1 text-[11.5px] font-medium text-white/45 transition-colors hover:bg-white/[0.06] hover:text-white/85 disabled:cursor-not-allowed disabled:opacity-40"
                          >
                            {pending?.kind === 'switch' && pending.id === account.id ? (
                              <Loader2 size={13} className="animate-spin" />
                            ) : (
                              'Switch'
                            )}
                          </button>
                          <button
                            type="button"
                            disabled={busy}
                            onClick={() => void remove(account.id)}
                            title={'Remove account'}
                            aria-label={`Remove ${label}`}
                            className="flex size-7 cursor-pointer items-center justify-center rounded-md text-white/35 transition-colors hover:bg-red-500/10 hover:text-red-400 disabled:cursor-not-allowed disabled:opacity-40"
                          >
                            {pending?.kind === 'remove' && pending.id === account.id ? (
                              <Loader2 size={14} className="animate-spin" />
                            ) : (
                              <Trash2 size={14} />
                            )}
                          </button>
                        </>
                      )}
                    </Row>
                  );
                })}
              </div>
            </div>

            <button
              type="button"
              disabled={busy}
              onClick={() => void add()}
              className="flex w-fit cursor-pointer items-center gap-2 rounded-md px-2.5 py-2 text-[12.5px] font-medium text-white/55 transition-colors hover:bg-white/[0.05] hover:text-white/85 disabled:cursor-not-allowed disabled:opacity-50"
            >
              {pending?.kind === 'add' ? (
                <Loader2 size={15} className="animate-spin" />
              ) : (
                <Plus size={15} />
              )}
              {pending?.kind === 'add' ? 'Waiting for sign-in...' : 'Add account'}
            </button>
          </>
        )}

        {error && (
          <p role="alert" className="text-[11.5px] leading-snug text-red-400">
            {error}
          </p>
        )}

        {accounts.length > 0 && <Divider />}

        <button
          type="button"
          disabled={busy}
          onClick={() => void signOut()}
          className="flex w-fit cursor-pointer items-center gap-2 rounded-md border border-red-500/10 bg-red-500/10 px-5 py-2.5 text-[13px] font-semibold text-red-400 transition-colors hover:border-red-500/20 hover:bg-red-500/20 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {pending?.kind === 'logout' && <Loader2 size={14} className="animate-spin" />}
          {'Sign Out'}
        </button>
      </div>
    </Card>
  );
}
