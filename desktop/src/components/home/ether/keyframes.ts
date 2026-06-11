/** Per-page keyframes «Эфира» (главная). Префикс eth- против коллизий;
 *  transform/opacity only. Орбы атмосферы берут tg-orb-drift из WaveFrame. */
export const ETHER_KEYFRAMES = `
@keyframes eth-pulse { 0%, 100% { opacity: 1; } 50% { opacity: 0.35; } }
@keyframes eth-eq { from { transform: scaleY(0.35); } to { transform: scaleY(1); } }
@media (prefers-reduced-motion: reduce) {
  .eth-anim { animation: none !important; }
}
`;
