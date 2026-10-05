// ── Direct mode base URLs ────────────────────────────────────
// All API traffic is served by the in-app Rust server (see direct/routes.rs).
// The port is assigned at runtime; `main.tsx` calls `setApiBase` before the
// first request. The values below are only a placeholder until then.
export let API_BASE = 'http://127.0.0.1:1';
export let API_STAR_BASE = API_BASE;
export let STREAMING_BASE = API_BASE;
export let STREAMING_PREMIUM_BASE = API_BASE;
export let STORAGE_BASE = API_BASE;
export let STORAGE_PREMIUM_BASE = API_BASE;
export let PAY_BASE = API_BASE;
/** Artwork is loaded straight from SoundCloud's public image CDN. */
export const IMAGES_BASE = 'https://i1.sndcdn.com';

export function setApiBase(port: number) {
  const base = `http://127.0.0.1:${port}`;
  API_BASE = base;
  API_STAR_BASE = base;
  STREAMING_BASE = base;
  STREAMING_PREMIUM_BASE = base;
  STORAGE_BASE = base;
  STORAGE_PREMIUM_BASE = base;
  PAY_BASE = base;
}

export const GITHUB_OWNER = 'zxcloli666';
export const GITHUB_REPO = 'SoundCloud-Desktop';
export const GITHUB_REPO_EN = 'SoundCloud-Desktop-EN';
export const APP_VERSION = __APP_VERSION__;

export const CHECK_UPDATES = true;
/** Direct mode: no backend, SoundCloud is queried by the app itself. */
export const DIRECT_MODE = true;

let _staticPort: number | null = null;
let _proxyPort: number | null = null;

export function setServerPorts(staticP: number, proxy: number) {
  _staticPort = staticP;
  _proxyPort = proxy;
}

export function getStaticPort(): number | null {
  return _staticPort;
}

export function getProxyPort(): number | null {
  return _proxyPort;
}
