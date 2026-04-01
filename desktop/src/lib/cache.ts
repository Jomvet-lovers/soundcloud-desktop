import { appCacheDir, join } from '@tauri-apps/api/path';
import { mkdir, readDir, remove, stat, writeFile } from '@tauri-apps/plugin-fs';
import { fetch as tauriFetch } from '@tauri-apps/plugin-http';
import { useSettingsStore } from '../stores/settings';
import { getStaticPort } from './constants';
import { trackedInvoke as invoke } from './diagnostics';

const ASSETS_DIR = 'assets';
const WALLPAPERS_DIR = 'wallpapers';
const IDLE_ASSETS_CLEAR_MS = 20 * 60 * 1000;
const CACHE_MAINTENANCE_INTERVAL_MS = 60 * 1000;

let cacheMaintenanceStarted = false;
let lastUserActivityAt = Date.now();
let assetsClearedDuringIdle = false;

/* ── Track cache (Rust) ─────────────────────────────────── */

export function isCached(urn: string): Promise<boolean> {
  return invoke<boolean>('track_is_cached', { urn });
}

export function getCacheFilePath(urn: string): Promise<string | null> {
  return invoke<string | null>('track_get_cache_path', { urn });
}

export function getCacheSize(): Promise<number> {
  return invoke<number>('track_cache_size');
}

export function clearCache(): Promise<void> {
  return invoke('track_clear_cache');
}

export function listCachedUrns(): Promise<string[]> {
  return invoke<string[]>('track_list_cached');
}

export function enforceAudioCacheLimit(
  limitMb = useSettingsStore.getState().audioCacheLimitMB,
): Promise<void> {
  if (!limitMb || limitMb <= 0) return Promise.resolve();
  return invoke('track_enforce_cache_limit', { limitMb });
}

/* ── Assets cache ────────────────────────────────────────── */

let assetsBasePath: string | null = null;

async function getAssetsDir(): Promise<string> {
  if (assetsBasePath) return assetsBasePath;
  const base = await appCacheDir();
  assetsBasePath = await join(base, ASSETS_DIR);
  await mkdir(assetsBasePath, { recursive: true });
  return assetsBasePath;
}

export async function getAssetsCacheSize(): Promise<number> {
  try {
    const dir = await getAssetsDir();
    const entries = await readDir(dir);
    let total = 0;
    for (const entry of entries) {
      if (entry.name) {
        const path = await join(dir, entry.name);
        const info = await stat(path);
        total += info.size;
      }
    }
    return total;
  } catch {
    return 0;
  }
}

export async function clearAssetsCache(): Promise<void> {
  try {
    const dir = await getAssetsDir();
    const entries = await readDir(dir);
    for (const entry of entries) {
      if (entry.name) {
        const path = await join(dir, entry.name);
        await remove(path).catch(() => {});
      }
    }
  } catch (e) {
    console.error('clearAssetsCache failed:', e);
  }
}

export function setupCacheMaintenance() {
  if (cacheMaintenanceStarted) return;
  cacheMaintenanceStarted = true;

  const markUserActive = () => {
    lastUserActivityAt = Date.now();
    assetsClearedDuringIdle = false;
  };

  for (const eventName of ['mousemove', 'mousedown', 'keydown', 'touchstart', 'focus']) {
    window.addEventListener(eventName, markUserActive, { passive: true });
  }

  void enforceAudioCacheLimit();

  useSettingsStore.subscribe((state, prev) => {
    if (state.audioCacheLimitMB !== prev.audioCacheLimitMB) {
      void enforceAudioCacheLimit(state.audioCacheLimitMB);
    }
  });

  window.setInterval(() => {
    void enforceAudioCacheLimit();

    if (assetsClearedDuringIdle) return;
    if (Date.now() - lastUserActivityAt < IDLE_ASSETS_CLEAR_MS) return;

    assetsClearedDuringIdle = true;
    void clearAssetsCache();
  }, CACHE_MAINTENANCE_INTERVAL_MS);
}

/* ── Wallpapers ──────────────────────────────────────────── */

let wallpapersBasePath: string | null = null;

async function getWallpapersDir(): Promise<string> {
  if (wallpapersBasePath) return wallpapersBasePath;
  const base = await appCacheDir();
  wallpapersBasePath = await join(base, WALLPAPERS_DIR);
  await mkdir(wallpapersBasePath, { recursive: true });
  return wallpapersBasePath;
}

function extensionFromType(mime: string): string {
  if (mime.includes('png')) return '.png';
  if (mime.includes('webp')) return '.webp';
  if (mime.includes('gif')) return '.gif';
  if (mime.includes('svg')) return '.svg';
  return '.jpg';
}

/** Скачивает картинку по URL и сохраняет в wallpapers/. Возвращает имя файла. */
export async function downloadWallpaper(url: string): Promise<string> {
  const res = await tauriFetch(url);
  if (!res.ok) throw new Error(`Download failed: ${res.status}`);
  const ct = res.headers.get('content-type') ?? 'image/jpeg';
  const ext = extensionFromType(ct);
  const name = `wallpaper_${Date.now()}${ext}`;
  const dir = await getWallpapersDir();
  const path = await join(dir, name);
  const buffer = await res.arrayBuffer();
  await writeFile(path, new Uint8Array(buffer));
  return name;
}

/** Сохраняет ArrayBuffer (из input type=file) как wallpaper. Возвращает имя файла. */
export async function saveWallpaperFromBuffer(
  buffer: ArrayBuffer,
  fileName: string,
): Promise<string> {
  const dir = await getWallpapersDir();
  const ext = fileName.includes('.') ? fileName.substring(fileName.lastIndexOf('.')) : '.jpg';
  const name = `wallpaper_${Date.now()}${ext}`;
  const path = await join(dir, name);
  await writeFile(path, new Uint8Array(buffer));
  return name;
}

/** Получить имена всех сохранённых wallpapers */
export async function listWallpapers(): Promise<string[]> {
  try {
    const dir = await getWallpapersDir();
    const entries = await readDir(dir);
    const names: string[] = [];
    for (const entry of entries) {
      if (entry.name && /\.(jpg|jpeg|png|webp|gif|svg)$/i.test(entry.name)) {
        names.push(entry.name);
      }
    }
    return names;
  } catch {
    return [];
  }
}

/** Удалить wallpaper по имени файла */
export async function removeWallpaper(name: string): Promise<void> {
  const dir = await getWallpapersDir();
  const path = await join(dir, name);
  await remove(path).catch(() => {});
}

/** HTTP URL для wallpaper по имени файла */
export function getWallpaperUrl(name: string): string | null {
  const port = getStaticPort();
  if (!port) return null;
  return `http://127.0.0.1:${port}/wallpapers/${encodeURIComponent(name)}`;
}

/* ── Track Download ──────────────────────────────────────── */

function sanitizeFilename(name: string): string {
  return name
    .replace(/[<>:"/\\|?*]/g, '_')
    .replace(/\s+/g, ' ')
    .trim();
}

export async function downloadTrack(urn: string, artist: string, title: string): Promise<string> {
  const { save } = await import('@tauri-apps/plugin-dialog');

  const filename = sanitizeFilename(`${artist} - ${title}.mp3`);

  const dest = await save({
    defaultPath: filename,
    filters: [{ name: 'Audio', extensions: ['mp3'] }],
  });
  if (!dest) throw new Error('cancelled');

  // Ensure cached via Rust
  let cachedPath = await getCacheFilePath(urn);
  if (!cachedPath) {
    // Force download via stream URL
    const { streamUrl, getSessionId } = await import('./api');
    const sessionId = getSessionId();
    const highQualityStreaming = useSettingsStore.getState().highQualityStreaming;
    try {
      await invoke('track_ensure_cached', {
        urn,
        url: streamUrl(urn, highQualityStreaming),
        sessionId,
      });
    } catch (error) {
      if (!highQualityStreaming) throw error;
      console.warn('[Cache] HQ download failed, retrying without hq:', error);
      await invoke('track_ensure_cached', {
        urn,
        url: streamUrl(urn, false),
        sessionId,
      });
    }
    cachedPath = await getCacheFilePath(urn);
  }
  if (!cachedPath) throw new Error('Failed to cache track');

  return invoke<string>('save_track_to_path', { cachePath: cachedPath, destPath: dest });
}
