// Единственное место знания о хостах: health-карта data-plane роутинга (30s cooldown),
// вердикты main/star/net + incident-state, probe-движок, выбор control-plane базы.
// Вердикт меняют только активные пробы и реальные успехи; пассивные фейлы лишь планируют пробу.
export {
  initHostStatus,
  isHealthy,
  markHealthy,
  markUnhealthy,
  noteMainAlive,
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
