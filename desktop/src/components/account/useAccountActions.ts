import { useCallback, useState } from 'react';
import { useAuthStore } from '../../stores/auth';

/** Which account action is in flight, for per-row spinners. */
export type PendingAccountAction =
  | { kind: 'switch'; id: number }
  | { kind: 'remove'; id: number }
  | { kind: 'add' }
  | { kind: 'logout' }
  | null;

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Shared pending/error plumbing for the account switcher surfaces (settings
 * card and sidebar menu). Actions resolve to `true` on success so callers can
 * close their popover.
 */
export function useAccountActions() {
  const switchAccount = useAuthStore((s) => s.switchAccount);
  const removeAccount = useAuthStore((s) => s.removeAccount);
  const addAccount = useAuthStore((s) => s.addAccount);
  const logout = useAuthStore((s) => s.logout);
  const [pending, setPending] = useState<PendingAccountAction>(null);
  const [error, setError] = useState<string | null>(null);

  const busy = pending !== null;

  const run = useCallback(
    async (
      action: Exclude<PendingAccountAction, null>,
      task: () => Promise<void>,
    ): Promise<boolean> => {
      if (busy) return false;
      setError(null);
      setPending(action);
      try {
        await task();
        return true;
      } catch (err) {
        setError(errorText(err));
        return false;
      } finally {
        setPending(null);
      }
    },
    [busy],
  );

  const switchTo = useCallback(
    (id: number) => run({ kind: 'switch', id }, () => switchAccount(id)),
    [run, switchAccount],
  );
  const remove = useCallback(
    (id: number) => run({ kind: 'remove', id }, () => removeAccount(id)),
    [run, removeAccount],
  );
  const add = useCallback(() => run({ kind: 'add' }, () => addAccount()), [run, addAccount]);
  const signOut = useCallback(() => run({ kind: 'logout' }, () => logout()), [run, logout]);
  const clearError = useCallback(() => setError(null), []);

  return { pending, error, busy, clearError, switchTo, remove, add, signOut };
}
