// Scoped keyframes for the STAR PASS page. Mounted once via <style> in StarPage.
// Idle-motion (foil sweep / scan line / halo / reveal) is gated per-component by
// usePerfMode(); these are just the keyframe definitions the animations reference.
export const STAR_KEYFRAMES = `
  @keyframes star-foil-sweep {
    0%   { background-position: 0% 50%; }
    100% { background-position: 300% 50%; }
  }
  @keyframes star-foil-text {
    0%   { background-position: 0% 50%; }
    100% { background-position: 200% 50%; }
  }
  @keyframes star-scan {
    0%, 100% { transform: translateY(0); }
    50%      { transform: translateY(var(--scan-travel, 196px)); }
  }
  @keyframes star-spin {
    to { transform: rotate(360deg); }
  }
  @keyframes star-blink {
    50% { opacity: 0; }
  }
  @keyframes star-halo {
    0%, 100% { opacity: 0.5; transform: translate3d(0, 0, 0); }
    50%      { opacity: 0.85; transform: translate3d(2%, 3%, 0); }
  }
  @keyframes star-reveal {
    from { opacity: 0; transform: translateY(20px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  @keyframes star-stamp-in {
    0%   { opacity: 0; transform: rotate(-4deg) scale(1.6); }
    60%  { opacity: 1; transform: rotate(-4deg) scale(0.92); }
    100% { opacity: 1; transform: rotate(-4deg) scale(1); }
  }
`;
