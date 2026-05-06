import { trackedInvoke } from './diagnostics';

export type CallStatus =
  | { kind: 'disabled' }
  | { kind: 'connecting' }
  | { kind: 'provisioning' }
  | { kind: 'active' }
  | { kind: 'failed'; error: string };

export const callIsEnabled = () => trackedInvoke<boolean>('call_is_enabled');

export const callStatus = () => trackedInvoke<CallStatus>('call_status');

export const callSetEnabled = (enabled: boolean) =>
  trackedInvoke<CallStatus>('call_set_enabled', { enabled });
