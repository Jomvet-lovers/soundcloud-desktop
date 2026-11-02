// The single place that knows about hosts: the data-plane routing health map (30s cooldown),
// the main/star/net verdicts + incident state, the probe engine, control-plane base choice.
// Only active probes and real successes change the verdict; passive failures merely schedule a probe.
// Separate from the verdicts lives main degradation — «answers `/health`, but does not serve».
export {
  isMainDegraded,
  markMainDegraded,
  noteMainBadResponse,
  SLOW_RESPONSE_MS,
  subscribeMainDegraded,
} from './degraded';
export {
  initHostStatus,
  isHealthy,
  isTimeoutError,
  markHealthy,
  markUnhealthy,
  noteMainAlive,
  noteRequestTimeout,
  requestProbe,
} from './probe';
export {
  type FailoverUi,
  getHostVerdict,
  type HostStatusState,
  type HostVerdict,
  isIncidentActive,
  type NetVerdict,
  preferredControlBase,
  selectFailoverUi,
  useHostStatusStore,
} from './store';
