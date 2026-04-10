export interface AuthConfig {
  nestUrl: string;
  nestToken: string;
  streamingUrl: string;
}

const STORAGE_KEY = "admin_auth";

export function getAuth(): AuthConfig | null {
  const raw = localStorage.getItem(STORAGE_KEY);
  if (!raw) return null;
  try {
    return JSON.parse(raw);
  } catch {
    return null;
  }
}

export function setAuth(config: AuthConfig) {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
}

export function clearAuth() {
  localStorage.removeItem(STORAGE_KEY);
}
