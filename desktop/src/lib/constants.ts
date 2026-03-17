// export const API_BASE = 'https://api.soundcloud.su';
export const API_BASE = 'http://localhost:3000';

export const GITHUB_OWNER = 'zxcloli666';
export const GITHUB_REPO = 'SoundCloud-Desktop';
export const GITHUB_REPO_EN = 'SoundCloud-Desktop-EN';
export const APP_VERSION = __APP_VERSION__;

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
