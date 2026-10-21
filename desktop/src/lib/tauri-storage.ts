import {
  BaseDirectory,
  copyFile,
  exists,
  mkdir,
  readTextFile,
  writeTextFile,
} from '@tauri-apps/plugin-fs';
import type { PersistStorage, StateStorage, StorageValue } from 'zustand/middleware';

const BASE_DIR = BaseDirectory.AppData;

// Persist-file renames: when a store's file name changes, older releases that
// still write the old name must not be able to touch the new file. On first
// read we transparently transplant the legacy content once and keep updating
// only the new name from then on.
const FILE_MIGRATIONS: Record<string, string> = {
  'sc-settings-v21': 'sc-settings',
};

// Keep the last written copy beside the settings file. Writing is not atomic
// in the schema sense: a version drop (older build) or a bad migration can
// replace the whole map, and the previous content is then unrecoverable.
// The copy is the pre-session file: one per process is enough, and copying it on
// every write cost an extra full-file IPC round-trip on each settings change.
const BACKUP_UNLESS_RENAMED_SUFFIX = '.bak';
const backedUpThisProcess = new Set<string>();

let dirReady: Promise<void> | null = null;

function ensureDir() {
  if (!dirReady) {
    dirReady = mkdir('', { baseDir: BASE_DIR, recursive: true }).catch(() => {});
  }
  return dirReady;
}

function filePath(name: string) {
  return `${name}.json`;
}

async function transplantLegacyFile(name: string, path: string): Promise<void> {
  const legacyName = FILE_MIGRATIONS[name];
  if (!legacyName) return;
  const legacyPath = filePath(legacyName);
  try {
    if (
      !(await exists(path, { baseDir: BASE_DIR })) &&
      (await exists(legacyPath, { baseDir: BASE_DIR }))
    ) {
      const legacy = await readTextFile(legacyPath, { baseDir: BASE_DIR });
      await writeTextFile(path, legacy, { baseDir: BASE_DIR });
    }
  } catch {
    // fall through: existing behavior (empty hydrate) still applies
  }
}

export const tauriStorage: StateStorage = {
  getItem: async (name) => {
    await ensureDir();
    const path = filePath(name);
    await transplantLegacyFile(name, path);
    try {
      if (await exists(path, { baseDir: BASE_DIR })) {
        return await readTextFile(path, { baseDir: BASE_DIR });
      }
    } catch {
      // first run or corrupted — treat as empty
    }
    return null;
  },

  setItem: async (name, value) => {
    await ensureDir();
    const path = filePath(name);
    if (name in FILE_MIGRATIONS && !backedUpThisProcess.has(name)) {
      backedUpThisProcess.add(name);
      try {
        if (await exists(path, { baseDir: BASE_DIR })) {
          await copyFile(path, `${path}${BACKUP_UNLESS_RENAMED_SUFFIX}`, {
            fromPathBaseDir: BASE_DIR,
            toPathBaseDir: BASE_DIR,
          });
        }
      } catch {
        // backup failure must not block the write
      }
    }
    try {
      await writeTextFile(path, value, { baseDir: BASE_DIR });
    } catch {
      // silently fail — don't break the app
    }
  },

  removeItem: async (name) => {
    const path = filePath(name);
    try {
      const { remove } = await import('@tauri-apps/plugin-fs');
      await remove(path, { baseDir: BASE_DIR });
    } catch {
      // file doesn't exist — ok
    }
  },
};

export function createThrottledJsonStorage<S>(delayMs = 500): PersistStorage<S> {
  const pending = new Map<string, StorageValue<S>>();
  const timers = new Map<string, ReturnType<typeof setTimeout>>();
  const lastWrite = new Map<string, number>();

  const write = (name: string) => {
    const value = pending.get(name);
    if (value === undefined) return;
    pending.delete(name);
    lastWrite.set(name, Date.now());
    void tauriStorage.setItem(name, JSON.stringify(value));
  };

  const flush = () => {
    for (const [name, timer] of timers) {
      clearTimeout(timer);
      timers.delete(name);
      write(name);
    }
  };

  if (typeof document !== 'undefined') {
    document.addEventListener('visibilitychange', () => {
      if (document.visibilityState === 'hidden') flush();
    });
    window.addEventListener('beforeunload', flush);
  }

  return {
    getItem: async (name) => {
      const raw = await tauriStorage.getItem(name);
      if (!raw) return null;
      try {
        return JSON.parse(raw) as StorageValue<S>;
      } catch {
        return null;
      }
    },

    setItem: (name, value) => {
      pending.set(name, value);
      if (timers.has(name)) return;
      const since = Date.now() - (lastWrite.get(name) ?? 0);
      if (since >= delayMs) {
        write(name);
        return;
      }
      timers.set(
        name,
        setTimeout(() => {
          timers.delete(name);
          write(name);
        }, delayMs - since),
      );
    },

    removeItem: (name) => {
      const timer = timers.get(name);
      if (timer) {
        clearTimeout(timer);
        timers.delete(name);
      }
      pending.delete(name);
      void tauriStorage.removeItem(name);
    },
  };
}
