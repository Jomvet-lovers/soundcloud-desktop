import React, { useCallback, useEffect, useRef } from 'react';
import { fetchAllUserLikedTracks, useUserLikedTracksPage } from '../../lib/hooks';
import { Pause, Play } from '../../lib/icons';
import { useIsPlayingFrom } from '../../lib/useTrackPlay';
import { usePlayerStore } from '../../stores/player';

/**
 * Play-all control for a profile's likes — a compact take on the round
 * track/playlist hero control (48px instead of 68px, since it sits in the
 * list, not next to a hero title). Rendered above the likes list, flush with
 * the rows' content edge.
 *
 * Only the first likes page is loaded; the rest of the profile's likes are
 * pulled in the background and appended while this queue stays active.
 */
export const ProfileLikesPlayButton = React.memo(function ProfileLikesPlayButton({
  urn,
}: {
  urn: string;
}) {
  // Same react-query key as the Likes tab's first page — shared cache, no
  // extra request.
  const q = useUserLikedTracksPage(urn, null);
  const tracks = q.data?.collection ?? [];

  // Union of every like we know of (first page + the background fill), so the
  // toggle stays accurate deeper into the queue.
  const likeUrnsRef = useRef<Set<string>>(new Set());
  useEffect(() => {
    for (const t of tracks) likeUrnsRef.current.add(t.urn);
  }, [tracks]);
  const isPlayingThis = useIsPlayingFrom(likeUrnsRef.current);

  const playAll = useCallback(() => {
    if (tracks.length === 0) return;
    const { play, pause, resume, currentTrack } = usePlayerStore.getState();
    if (isPlayingThis) {
      pause();
      return;
    }
    if (currentTrack && likeUrnsRef.current.has(currentTrack.urn)) {
      resume();
      return;
    }
    play(tracks[0], tracks);
    const started = usePlayerStore.getState().queue;
    void fetchAllUserLikedTracks(urn)
      .then((all) => {
        for (const t of all) likeUrnsRef.current.add(t.urn);
        const st = usePlayerStore.getState();
        if (st.queue !== started) return;
        const queued = new Set(st.queue.map((t) => t.urn));
        const rest = all.filter((t) => !queued.has(t.urn));
        if (rest.length > 0) st.addToQueue(rest);
      })
      .catch(() => {});
  }, [tracks, isPlayingThis, urn]);

  return (
    <button
      type="button"
      onClick={playAll}
      aria-label={isPlayingThis ? 'Pause' : 'Play'}
      className="w-[48px] h-[48px] shrink-0 rounded-full border border-white/[0.18] hover:border-white/[0.4] flex items-center justify-center text-white/90 hover:text-white transition-colors cursor-pointer"
    >
      {isPlayingThis ? (
        <span key="pause" className="animate-icon-pop flex items-center justify-center">
          <Pause size={18} fill="currentColor" strokeWidth={0} />
        </span>
      ) : (
        <span key="play" className="animate-icon-pop flex items-center justify-center">
          <Play size={18} fill="currentColor" strokeWidth={0} className="ml-0.5" />
        </span>
      )}
    </button>
  );
});
