import { toast } from 'sonner';
import { create } from 'zustand';
import type { Track } from '../stores/player';
import { ensureTrackCached, listCachedUrns } from './cache';

const CONCURRENCY = 4;

export interface BulkProgress {
  done: number;
  total: number;
  failed: number;
  /** true while we are fetching track list / computing pending */
  preparing: boolean;
}

export interface BulkLabels {
  success: (n: number) => string;
  failed: (n: number) => string;
  allCached: () => string;
}

interface State {
  entries: Record<string, BulkProgress>;
  start(key: string, getTracks: () => Track[] | Promise<Track[]>, labels: BulkLabels): void;
  cancel(key: string): void;
}

const controllers = new Map<string, AbortController>();

const emptyProgress = (): BulkProgress => ({ done: 0, total: 0, failed: 0, preparing: true });

export const useBulkCacheStore = create<State>((set, get) => ({
  entries: {},

  cancel(key) {
    controllers.get(key)?.abort();
  },

  start(key, getTracks, labels) {
    if (get().entries[key]) return;

    const abort = new AbortController();
    controllers.set(key, abort);
    set((s) => ({ entries: { ...s.entries, [key]: emptyProgress() } }));

    const clear = () => {
      controllers.delete(key);
      set((s) => {
        const next = { ...s.entries };
        delete next[key];
        return { entries: next };
      });
    };

    void (async () => {
      let done = 0;
      let failed = 0;
      try {
        const [tracks, cachedUrns] = await Promise.all([
          Promise.resolve(getTracks()),
          listCachedUrns(),
        ]);
        if (abort.signal.aborted) return;

        const cached = new Set(cachedUrns);
        const pending = tracks.filter((t) => !!t?.urn && !cached.has(t.urn));

        if (pending.length === 0) {
          toast.success(labels.allCached());
          return;
        }

        const total = pending.length;
        set((s) => ({ entries: { ...s.entries, [key]: { done: 0, total, failed: 0, preparing: false } } }));

        const queue = pending.slice();
        const worker = async () => {
          while (!abort.signal.aborted) {
            const track = queue.shift();
            if (!track) break;
            try {
              await ensureTrackCached(track.urn);
            } catch (e) {
              failed++;
              console.error('[bulkCache]', track.urn, e);
            }
            done++;
            set((s) => ({
              entries: { ...s.entries, [key]: { done, total, failed, preparing: false } },
            }));
          }
        };

        await Promise.all(Array.from({ length: CONCURRENCY }, () => worker()));

        if (abort.signal.aborted) return;
        if (failed > 0) toast.error(labels.failed(failed));
        else toast.success(labels.success(done));
      } catch (e) {
        console.error('[bulkCache] start failed', e);
      } finally {
        clear();
      }
    })();
  },
}));
