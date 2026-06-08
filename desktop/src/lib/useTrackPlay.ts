import { useCallback, useEffect, useRef } from 'react';
import { type Track, usePlayerStore } from '../stores/player';
import { rememberTracks } from './offline-index';

/**
 * Optimized hook for track play/pause.
 * Only re-renders when THIS track's play state changes, not on every global isPlaying toggle.
 *
 * `queue` may be a thunk `() => Track[]` so a large grid can pass a STABLE prop
 * (and not defeat each tile's React.memo) while still resolving the live queue
 * lazily at play time.
 *
 * `onPlay` fires right AFTER a NEW track starts (the play branch, not resume) —
 * used to arm a queue-continuation source (см. lib/queue-continuation.ts), напр.
 * «лайки до конца». Pass a STABLE ref to keep memo'd tiles happy.
 */
export function useTrackPlay(track: Track, queue?: Track[] | (() => Track[]), onPlay?: () => void) {
  const isThis = usePlayerStore((s) => s.currentTrack?.urn === track.urn);
  const isThisPlaying = usePlayerStore((s) => s.currentTrack?.urn === track.urn && s.isPlaying);

  const trackRef = useRef(track);
  const queueRef = useRef(queue);
    const onPlayRef = useRef(onPlay);
  trackRef.current = track;
  queueRef.current = queue;
    onPlayRef.current = onPlay;

  useEffect(() => {
    void rememberTracks([track]);
  }, [track]);

  const togglePlay = useCallback(() => {
    const { play, pause, resume } = usePlayerStore.getState();
    if (isThisPlaying) pause();
    else if (isThis) resume();
    else {
        const q = queueRef.current;
        const resolved = typeof q === 'function' ? q() : q;
        play(trackRef.current, resolved?.length ? resolved : [trackRef.current]);
        onPlayRef.current?.();
    }
  }, [isThis, isThisPlaying]);

  return { isThis, isThisPlaying, togglePlay };
}

/**
 * Check if any track from a set of URNs is currently playing.
 * Only re-renders when the result changes.
 */
export function useIsPlayingFrom(trackUrns: Set<string>) {
  return usePlayerStore(
    (s) => s.isPlaying && s.currentTrack != null && trackUrns.has(s.currentTrack.urn),
  );
}
