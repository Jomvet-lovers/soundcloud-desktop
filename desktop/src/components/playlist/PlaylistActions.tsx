import { useQuery, useQueryClient } from '@tanstack/react-query';
import React, { useEffect, useRef, useState } from 'react';
import { api } from '../../lib/api';
import { fc } from '../../lib/formatters';
import type { Playlist } from '../../lib/hooks';
import { Check, Heart, LinkIcon, MapPin, Shuffle, Trash2 } from '../../lib/icons';
import { usePulseHeart } from '../../lib/pulse-heart';
import { SharingToggle } from '../music/SharingToggle';

const PlaylistLikeBtn = React.memo(function PlaylistLikeBtn({
  playlistUrn,
  count,
}: {
  playlistUrn: string;
  count?: number;
}) {
  const { data: likeStatus } = useQuery({
    queryKey: ['likes', 'playlist', playlistUrn],
    queryFn: () => api<{ liked: boolean }>(`/likes/playlists/${encodeURIComponent(playlistUrn)}`),
    staleTime: 1000 * 60 * 5,
  });
  const [liked, setLiked] = useState(false);
  const [localCount, setLocalCount] = useState(count ?? 0);
  const { shownLiked, pulse, heartRef, pillRef } = usePulseHeart(liked);
  const qc = useQueryClient();
  const invalidateTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (likeStatus) setLiked(likeStatus.liked);
  }, [likeStatus]);
  useEffect(() => setLocalCount(count ?? 0), [count]);
  useEffect(() => () => clearTimeout(invalidateTimer.current ?? undefined), []);

  const toggle = async () => {
    const next = !liked;
    pulse(next);
    setLiked(next);
    setLocalCount((c) => c + (next ? 1 : -1));
    try {
      await api(`/likes/playlists/${encodeURIComponent(playlistUrn)}`, {
        method: next ? 'POST' : 'DELETE',
      });
      invalidateTimer.current = setTimeout(() => {
        qc.invalidateQueries({ queryKey: ['likes', 'playlist', playlistUrn] });
        qc.invalidateQueries({ queryKey: ['me', 'likes', 'playlists'] });
      }, 3000);
    } catch {
      setLiked(!next);
      setLocalCount((c) => c + (next ? -1 : 1));
    }
  };

  return (
    <button
      ref={pillRef as React.RefObject<HTMLButtonElement | null>}
      type="button"
      onClick={toggle}
      title={'likes'}
      className={`inline-flex items-center gap-1.5 h-10 px-3 rounded-md text-[12.5px] font-medium tabular-nums transition-colors cursor-pointer ${
        shownLiked ? 'text-accent' : 'text-white/60 hover:text-white hover:bg-white/[0.06]'
      }`}
    >
      <span
        ref={heartRef as React.RefObject<HTMLSpanElement | null>}
        className="flex items-center justify-center"
      >
        <Heart size={15} fill={shownLiked ? 'currentColor' : 'none'} />
      </span>
      <span>{fc(localCount)}</span>
    </button>
  );
});

const CopyIconAction = React.memo(function CopyIconAction({ url }: { url?: string }) {
  const [copied, setCopied] = useState(false);
  const copyTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => clearTimeout(copyTimer.current ?? undefined), []);
  if (!url) return null;
  const copy = () => {
    try {
      const u = new URL(url);
      for (const p of ['utm_medium', 'utm_campaign', 'utm_source']) u.searchParams.delete(p);
      navigator.clipboard.writeText(u.toString().replace(/\?$/, ''));
    } catch {
      navigator.clipboard.writeText(url);
    }
    setCopied(true);
    copyTimer.current = setTimeout(() => setCopied(false), 1600);
  };
  return (
    <button
      type="button"
      onClick={copy}
      title={copied ? 'Copied!' : 'Copy link'}
      aria-label={copied ? 'Copied!' : 'Copy link'}
      className={`inline-flex items-center justify-center size-10 rounded-md transition-colors cursor-pointer ${
        copied ? 'text-emerald-400' : 'text-white/60 hover:text-white hover:bg-white/[0.06]'
      }`}
    >
      {copied ? <Check size={16} /> : <LinkIcon size={16} />}
    </button>
  );
});

/** The crate's control bar — Shuffle, Pin, Like + utility rail (the play
 *  control lives next to the title, like the track page). */
export const PlaylistActions = React.memo(function PlaylistActions({
  playlist,
  isOwner,
  isPinned,
  onShuffle,
  onTogglePin,
  onDelete,
}: {
  playlist: Playlist;
  isOwner: boolean;
  isPinned: boolean;
  onShuffle: () => void;
  onTogglePin: () => void;
  onDelete: () => void;
}) {
  return (
    <div className="flex items-center gap-3 flex-wrap justify-center lg:justify-start">
      <button
        type="button"
        onClick={onShuffle}
        title={'Shuffle'}
        className="inline-flex items-center gap-1.5 h-10 px-3 rounded-md text-[12.5px] font-medium text-white/60 hover:text-white hover:bg-white/[0.06] transition-colors cursor-pointer"
      >
        <Shuffle size={14} />
        <span className="hidden sm:inline">{'Shuffle'}</span>
      </button>

      <PlaylistLikeBtn playlistUrn={playlist.urn} count={playlist.likes_count} />

      <div className="flex items-center gap-0.5">
        <button
          type="button"
          onClick={onTogglePin}
          title={isPinned ? 'Unpin playlist' : 'Pin playlist'}
          aria-label={isPinned ? 'Unpin playlist' : 'Pin playlist'}
          className={`inline-flex items-center justify-center size-10 rounded-md transition-colors cursor-pointer ${
            isPinned ? 'text-accent' : 'text-white/60 hover:text-white hover:bg-white/[0.06]'
          }`}
        >
          <MapPin size={16} />
        </button>
        <CopyIconAction url={playlist.permalink_url} />
        {isOwner && (
          <>
            <SharingToggle kind="playlist" urn={playlist.urn} sharing={playlist.sharing} />
            <button
              type="button"
              onClick={onDelete}
              title={'Delete playlist'}
              aria-label={'Delete playlist'}
              className="inline-flex items-center justify-center size-10 rounded-md text-white/55 hover:text-red-400 hover:bg-red-500/10 transition-colors cursor-pointer"
            >
              <Trash2 size={16} />
            </button>
          </>
        )}
      </div>
    </div>
  );
});
