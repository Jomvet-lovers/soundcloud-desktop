import type { Account } from '../../stores/auth';

/** Display name for an account (SoundCloud usernames can be empty). */
export function accountName(account: Pick<Account, 'username' | 'id'>): string {
  return account.username.trim() || `Account ${account.id}`;
}

/** Active account first; the rest keep the order Rust sent. */
export function orderedAccounts(accounts: Account[], activeId: number | null): Account[] {
  const active = accounts.find((account) => account.id === activeId);
  return active ? [active, ...accounts.filter((account) => account.id !== active.id)] : accounts;
}
