import { useQueryClient } from '@tanstack/react-query';
import React, { useEffect } from 'react';
import { api } from '../../lib/api';
import { invalidateAllLikesCache } from '../../lib/hooks';
import { Heart } from '../../lib/icons';
import { optimisticToggleLike, setLikedUrn, useLiked } from '../../lib/likes';
import { usePulseHeart } from '../../lib/pulse-heart';
import type { Track } from '../../stores/player';

export const LikeButton = React.memo(function LikeButton({
  track,
  variant = 'inline',
}: {
  track: Track;
  variant?: 'chip' | 'inline' | 'bar';
}) {
  const liked = useLiked(track.urn);
  const { shownLiked, pulse, heartRef, pillRef } = usePulseHeart(liked);

  // Seed from API data when available
  useEffect(() => {
    if (track.user_favorite) setLikedUrn(track.urn, true);
  }, [track.urn, track.user_favorite]);
  const qc = useQueryClient();

  const toggle = async (e: React.MouseEvent) => {
    e.stopPropagation();
    const next = !liked;
    pulse(next);
    optimisticToggleLike(qc, track, next);
    invalidateAllLikesCache();
    try {
      await api(`/likes/tracks/${encodeURIComponent(track.urn)}`, {
        method: next ? 'POST' : 'DELETE',
        body: next ? JSON.stringify(track) : undefined,
      });
    } catch {
      optimisticToggleLike(qc, track, !next);
    }
  };

  if (variant === 'chip') {
    return (
      <button
        ref={pillRef as React.RefObject<HTMLButtonElement | null>}
        type="button"
        onClick={toggle}
        className={`cursor-pointer w-6 h-6 rounded-full flex items-center justify-center shrink-0 opacity-0 group-hover:opacity-100 transition-all duration-200 ${
          shownLiked
            ? 'bg-accent/80 text-accent-contrast'
            : 'bg-black/50 text-white/80 hover:text-white hover:bg-black/70'
        }`}
      >
        <span
          ref={heartRef as React.RefObject<HTMLSpanElement | null>}
          className="flex items-center justify-center"
        >
          <Heart size={12} fill={shownLiked ? 'currentColor' : 'none'} />
        </span>
      </button>
    );
  }

  if (variant === 'bar') {
    return (
      <button
        ref={pillRef as React.RefObject<HTMLButtonElement | null>}
        type="button"
        onClick={toggle}
        title={shownLiked ? 'Unlike' : 'Like'}
        className={`cursor-pointer w-8 h-8 rounded-full flex items-center justify-center shrink-0 transition-colors ${
          shownLiked ? 'text-accent hover:text-accent-hover' : 'text-white/55 hover:text-white/90'
        }`}
      >
        <span
          ref={heartRef as React.RefObject<HTMLSpanElement | null>}
          className="flex items-center justify-center"
        >
          <Heart size={15} fill={shownLiked ? 'currentColor' : 'none'} />
        </span>
      </button>
    );
  }

  return (
    <button
      ref={pillRef as React.RefObject<HTMLButtonElement | null>}
      type="button"
      onClick={toggle}
      className={`cursor-pointer w-8 h-8 rounded-lg flex items-center justify-center opacity-0 group-hover:opacity-100 transition-all duration-200 shrink-0 ${
        shownLiked ? 'text-accent' : 'text-white/20 hover:text-white/50'
      }`}
    >
      <span
        ref={heartRef as React.RefObject<HTMLSpanElement | null>}
        className="flex items-center justify-center"
      >
        <Heart size={14} fill={shownLiked ? 'currentColor' : 'none'} />
      </span>
    </button>
  );
});
