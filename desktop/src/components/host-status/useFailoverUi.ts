import {useShallow} from 'zustand/shallow';
import {type FailoverUi, selectFailoverUi, useHostStatusStore} from '../../lib/host-status';
import {usePremium} from '../../lib/premium-cache';
import {useAppStatusStore} from '../../stores/app-status';
import {useAuthStore} from '../../stores/auth';

/** Derived failover-UI из вердиктов хостов + premium/session/connectivity-гейтов. */
export function useFailoverUi(): FailoverUi {
  const verdicts = useHostStatusStore(
    useShallow((s) => ({ main: s.main, star: s.star, net: s.net })),
  );
  const navigatorOnline = useAppStatusStore((s) => s.navigatorOnline);
  const offlineBypass = useAppStatusStore((s) => s.offlineBypass);
  const hasSession = useAuthStore((s) => s.hasSession);
  const premium = usePremium();
  return selectFailoverUi(verdicts, premium, hasSession, navigatorOnline, offlineBypass);
}
