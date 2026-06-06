/* Injected once on the Library "Sound Print" page. Supplies the orb-drift the
 * reused search Atmosphere references (not global), plus the soundprint's own
 * motion: columns rising on load, breathing while idle, the artwork wall drifting
 * behind the frost. Transform/opacity only — idle motion gates on perf.idleAnim. */
export const LIBRARY_KEYFRAMES = `
@keyframes tg-orb-drift {
  0%   { transform: translate3d(0,0,0) scale(1); }
  33%  { transform: translate3d(3%,4%,0) scale(1.08); }
  66%  { transform: translate3d(-3%,2%,0) scale(1.04); }
  100% { transform: translate3d(0,0,0) scale(1); }
}
@keyframes tg-orb-drift-lite {
  0%   { transform: translate3d(0,0,0); }
  33%  { transform: translate3d(3%,4%,0); }
  66%  { transform: translate3d(-3%,2%,0); }
  100% { transform: translate3d(0,0,0); }
}
@keyframes sp-rise {
  from { opacity: 0; transform: translateY(16px); }
  to   { opacity: 1; transform: translateY(0); }
}
@keyframes sp-breathe {
  0%, 100% { transform: scaleY(0.93); }
  50%      { transform: scaleY(1); }
}
@keyframes sp-eq {
  0%, 100% { transform: scaleY(0.3); }
  50%      { transform: scaleY(1); }
}
@keyframes sp-mosaic {
  from { transform: translate3d(0,0,0); }
  to   { transform: translate3d(-4%,-2%,0); }
}
[data-app-hidden='1'] .sp-breathe,
[data-app-hidden='1'] .sp-eq,
[data-app-hidden='1'] .sp-mosaic,
[data-app-hidden='1'] .tg-orb { animation-play-state: paused !important; }
@media (prefers-reduced-motion: reduce) {
  .sp-rise, .sp-breathe, .sp-eq, .sp-mosaic { animation: none !important; }
  .sp-rise { opacity: 1 !important; transform: none !important; }
  .sp-breathe { transform: none !important; }
}
`;
