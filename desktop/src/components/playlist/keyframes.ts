/* Injected once on the playlist page. Supplies the keyframes the reused
 * Atmosphere (tg-orb-drift) and GlassHeroPanel (hub-rise) expect, plus the
 * crate's own deal-in. Transform/opacity only. */
export const PLAYLIST_KEYFRAMES = `
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
@keyframes hub-rise {
  from { opacity: 0; transform: translateY(18px) scale(0.99); }
  to   { opacity: 1; transform: translateY(0) scale(1); }
}
@keyframes crate-deal-in {
  from { opacity: 0; transform: translateY(26px); }
  to   { opacity: 1; transform: translateY(0); }
}
.crate-sleeve-in { animation: crate-deal-in 620ms cubic-bezier(0.2,0.8,0.2,1) both; }
[data-app-hidden='1'] .crate-sleeve-in { animation: none !important; }
@media (prefers-reduced-motion: reduce) {
  .crate-sleeve-in { animation: none !important; opacity: 1 !important; transform: none !important; }
}
`;
