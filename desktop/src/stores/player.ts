import { create } from 'zustand';
import { persist, createJSONStorage } from 'zustand/middleware';
import { tauriStorage } from '../lib/tauri-storage';

export interface Track {
  id: number;
  urn: string;
  title: string;
  duration: number;
  artwork_url: string | null;
  permalink_url?: string;
  waveform_url?: string;
  genre?: string;
  tag_list?: string;
  description?: string;
  created_at?: string;
  comment_count?: number;
  playback_count?: number;
  likes_count?: number;
  favoritings_count?: number;
  reposts_count?: number;
  user_favorite?: boolean;
  user: {
    id: number;
    urn: string;
    username: string;
    avatar_url: string;
    permalink_url: string;
  };
}

type RepeatMode = 'off' | 'one' | 'all';

interface PlayerState {
  currentTrack: Track | null;
  queue: Track[];
  queueIndex: number;
  isPlaying: boolean;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;

  play: (track: Track, queue?: Track[]) => void;
  pause: () => void;
  resume: () => void;
  togglePlay: () => void;
  next: () => void;
  prev: () => void;
  setVolume: (v: number) => void;
  setQueue: (queue: Track[]) => void;
  addToQueue: (tracks: Track[]) => void;
  removeFromQueue: (index: number) => void;
  moveInQueue: (from: number, to: number) => void;
  clearQueue: () => void;
  toggleShuffle: () => void;
  toggleRepeat: () => void;
}

export const usePlayerStore = create<PlayerState>()(
  persist(
    (set, get) => ({
      currentTrack: null,
      queue: [],
      queueIndex: -1,
      isPlaying: false,
      volume: 50,
      shuffle: false,
      repeat: 'off',

      play: (track, queue) => {
        if (queue) {
          const idx = queue.findIndex((t) => t.urn === track.urn);
          set({
            currentTrack: track,
            queue,
            queueIndex: idx >= 0 ? idx : 0,
            isPlaying: true,
          });
        } else {
          const { queue: currentQueue } = get();
          set({
            currentTrack: track,
            queue: [...currentQueue, track],
            queueIndex: currentQueue.length,
            isPlaying: true,
          });
        }
      },

      pause: () => set({ isPlaying: false }),
      resume: () => set({ isPlaying: true }),

      togglePlay: () => {
        const { isPlaying, currentTrack } = get();
        if (currentTrack) set({ isPlaying: !isPlaying });
      },

      next: () => {
        const { queue, queueIndex, repeat, shuffle } = get();
        if (queue.length === 0) return;

        let nextIdx: number;
        if (shuffle) {
          nextIdx = Math.floor(Math.random() * queue.length);
        } else {
          nextIdx = queueIndex + 1;
        }

        if (nextIdx >= queue.length) {
          if (repeat === 'all') nextIdx = 0;
          else {
            set({ isPlaying: false });
            return;
          }
        }

        set({
          currentTrack: queue[nextIdx],
          queueIndex: nextIdx,
          isPlaying: true,
        });
      },

      prev: () => {
        const { queue, queueIndex } = get();
        const prevIdx = Math.max(0, queueIndex - 1);
        set({
          currentTrack: queue[prevIdx],
          queueIndex: prevIdx,
          isPlaying: true,
        });
      },

      setVolume: (v) => set({ volume: Math.round(Math.max(0, Math.min(200, v))) }),

      setQueue: (queue) =>
        set((s) => {
          const idx = s.currentTrack ? queue.findIndex((t) => t.urn === s.currentTrack!.urn) : -1;
          return { queue, queueIndex: idx >= 0 ? idx : s.queueIndex };
        }),

      addToQueue: (tracks) => set((s) => ({ queue: [...s.queue, ...tracks] })),

      removeFromQueue: (index) =>
        set((s) => {
          const queue = s.queue.filter((_, i) => i !== index);
          const queueIndex =
            index < s.queueIndex
              ? s.queueIndex - 1
              : index === s.queueIndex
                ? Math.min(s.queueIndex, queue.length - 1)
                : s.queueIndex;
          return { queue, queueIndex };
        }),

      moveInQueue: (from, to) =>
        set((s) => {
          const queue = [...s.queue];
          const [item] = queue.splice(from, 1);
          queue.splice(to, 0, item);
          let queueIndex = s.queueIndex;
          if (s.queueIndex === from) queueIndex = to;
          else if (from < s.queueIndex && to >= s.queueIndex) queueIndex--;
          else if (from > s.queueIndex && to <= s.queueIndex) queueIndex++;
          return { queue, queueIndex };
        }),

      clearQueue: () => set({ queue: [], queueIndex: -1 }),
      toggleShuffle: () => set((s) => ({ shuffle: !s.shuffle })),
      toggleRepeat: () =>
        set((s) => ({
          repeat: s.repeat === 'off' ? 'all' : s.repeat === 'all' ? 'one' : 'off',
        })),
    }),
    {
      name: 'sc-player',
      storage: createJSONStorage(() => tauriStorage),
      version: 3,
      partialize: (state) => ({
        volume: state.volume,
        currentTrack: state.currentTrack,
        queue: state.queue,
        queueIndex: state.queueIndex,
        shuffle: state.shuffle,
        repeat: state.repeat,
      }),
    },
  ),
);
