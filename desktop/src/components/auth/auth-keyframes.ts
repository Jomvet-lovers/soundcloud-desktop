/** Login-only motion: sonar pulses out of the logo, a slow float, a shine sweep
 *  across the CTA, and the backdrop equalizer. Transform/opacity only. */
export const AUTH_KEYFRAMES = `
@keyframes auth-sonar { 0% { transform: scale(0.7); opacity: 0.5; } 100% { transform: scale(2.5); opacity: 0; } }
@keyframes auth-float { 0%, 100% { transform: translateY(0); } 50% { transform: translateY(-6px); } }
@keyframes auth-shine { 0% { transform: translateX(-130%) skewX(-18deg); } 60%, 100% { transform: translateX(240%) skewX(-18deg); } }
@keyframes auth-eq { 0%, 100% { transform: scaleY(0.28); } 50% { transform: scaleY(1); } }
@media (prefers-reduced-motion: reduce) {
  .auth-anim { animation: none !important; }
}
`;
