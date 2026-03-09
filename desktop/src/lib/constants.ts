export const API_BASE = 'https://backend.soundcloud.work.gd';
// export const API_BASE = 'http://localhost:3000';

export const GITHUB_OWNER = 'zxcloli666';
export const GITHUB_REPO = 'SoundCloud-Desktop';
export const APP_VERSION = __APP_VERSION__;

let _audioPort: number | null = null;
let _proxyPort: number | null = null;

export function setServerPorts(audio: number, proxy: number) {
  _audioPort = audio;
  _proxyPort = proxy;
}

export function getAudioPort(): number | null {
  return _audioPort;
}

export function getProxyPort(): number | null {
  return _proxyPort;
}
