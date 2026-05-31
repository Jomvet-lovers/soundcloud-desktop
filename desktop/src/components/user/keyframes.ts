export const USER_PAGE_KEYFRAMES = `
  @keyframes prismatic-shift {
    0% { background-position: 0% 50%; }
    100% { background-position: 200% 50%; }
  }
  @keyframes orb-drift {
    0%   { transform: translate3d(0, 0, 0) scale(1); }
    33%  { transform: translate3d(3%, 4%, 0) scale(1.08); }
    66%  { transform: translate3d(-3%, 2%, 0) scale(1.04); }
    100% { transform: translate3d(0, 0, 0) scale(1); }
  }
  @keyframes orb-drift-lite {
    0%   { transform: translate3d(0, 0, 0); }
    33%  { transform: translate3d(3%, 4%, 0); }
    66%  { transform: translate3d(-3%, 2%, 0); }
    100% { transform: translate3d(0, 0, 0); }
  }
  @keyframes ring-rotate {
    from { transform: rotate(0deg); }
    to   { transform: rotate(360deg); }
  }
  @keyframes hub-rise {
    from { opacity: 0; transform: translateY(24px); filter: blur(8px); }
    to   { opacity: 1; transform: translateY(0);    filter: blur(0); }
  }
  @keyframes star-twinkle {
    0%   { opacity: var(--min, 0.2); transform: scale(0.85); }
    50%  { opacity: var(--max, 0.9); transform: scale(1.05); }
    100% { opacity: var(--min, 0.2); transform: scale(0.85); }
  }
`;
