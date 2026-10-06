import { useQueryClient } from '@tanstack/react-query';
import type React from 'react';
import { useEffect, useState } from 'react';
import { createPortal } from 'react-dom';
import { useNavigate } from 'react-router-dom';
import { toast } from 'sonner';
import { api } from '../../lib/api';
import { invalidateAllLikesCache } from '../../lib/hooks';
import { Heart, LinkIcon, ListPlus, Plus, User } from '../../lib/icons';
import { optimisticToggleLike, setLikedUrn, useLiked } from '../../lib/likes';
import type { Track } from '../../stores/player';
import { usePlayerStore } from '../../stores/player';
import { type UserMenuTarget, useTrackMenuStore } from '../../stores/track-menu';
import { AddToPlaylistDialog } from './AddToPlaylistDialog';

/** Rough menu box, used to keep it inside the viewport. */
const MENU_W = 236;
const MENU_H = 200;

function MenuItem({
  icon,
  label,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      onClick={onClick}
      className="w-full flex items-center gap-3 px-3 h-10 rounded-lg text-[13px] font-medium text-white/80 hover:text-white hover:bg-white/[0.06] transition-colors cursor-pointer"
    >
      <span className="shrink-0 text-white/50">{icon}</span>
      <span className="truncate">{label}</span>
    </button>
  );
}

/** Shared chrome: backdrop + Escape/scroll dismissal + clamped position. */
function MenuShell({ children }: { children: React.ReactNode }) {
  const x = useTrackMenuStore((s) => s.x);
  const y = useTrackMenuStore((s) => s.y);
  const close = useTrackMenuStore((s) => s.close);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close();
    };
    const onScroll = () => close();
    document.addEventListener('keydown', onKey, true);
    window.addEventListener('scroll', onScroll, true);
    return () => {
      document.removeEventListener('keydown', onKey, true);
      window.removeEventListener('scroll', onScroll, true);
    };
  }, [close]);

  const left = Math.max(8, Math.min(x, window.innerWidth - MENU_W - 8));
  const top = Math.max(8, Math.min(y, window.innerHeight - MENU_H - 8));

  return (
    <>
      <div
        className="fixed inset-0 z-[95]"
        onMouseDown={close}
        onContextMenu={(e) => {
          e.preventDefault();
          close();
        }}
      />
      <div
        role="menu"
        className="fixed z-[96] w-[236px] rounded-xl border border-white/[0.1] bg-[#141417] p-1.5 shadow-[0_16px_40px_rgba(0,0,0,0.5)]"
        style={{ left, top }}
      >
        {children}
      </div>
    </>
  );
}

function TrackMenu({ track, onAddToPlaylist }: { track: Track; onAddToPlaylist: () => void }) {
  const close = useTrackMenuStore((s) => s.close);
  const liked = useLiked(track.urn);
  const qc = useQueryClient();
  const addToQueueNext = usePlayerStore((s) => s.addToQueueNext);

  useEffect(() => {
    if (track.user_favorite) setLikedUrn(track.urn, true);
  }, [track]);

  const toggleLike = () => {
    const next = !liked;
    optimisticToggleLike(qc, track, next);
    invalidateAllLikesCache();
    void api(`/likes/tracks/${encodeURIComponent(track.urn)}`, {
      method: next ? 'POST' : 'DELETE',
      body: next ? JSON.stringify(track) : undefined,
    }).catch(() => optimisticToggleLike(qc, track, !next));
    close();
  };

  const addNext = () => {
    addToQueueNext([track]);
    close();
  };

  const share = async () => {
    close();
    let url = track.permalink_url;
    if (!url) {
      // History rows only carry the bare id — pull the real permalink.
      try {
        const fresh = await api<Track>(`/tracks/${encodeURIComponent(track.urn)}`);
        url = fresh.permalink_url;
      } catch {
        // fall through to the error toast
      }
    }
    if (!url) {
      toast.error('No link available');
      return;
    }
    try {
      await navigator.clipboard.writeText(url);
      toast.success('Copied!');
    } catch {
      toast.error('Something went wrong');
    }
  };

  return (
    <MenuShell>
      <MenuItem
        icon={<Heart size={15} fill={liked ? 'currentColor' : 'none'} />}
        label={liked ? 'Remove from library' : 'Add to library'}
        onClick={toggleLike}
      />
      <MenuItem icon={<ListPlus size={15} />} label={'Add to Next up'} onClick={addNext} />
      <MenuItem
        icon={<Plus size={15} />}
        label={'Add to playlist'}
        onClick={() => {
          close();
          onAddToPlaylist();
        }}
      />
      <div className="my-1 h-px bg-white/[0.06]" />
      <MenuItem icon={<LinkIcon size={15} />} label={'Share'} onClick={share} />
    </MenuShell>
  );
}

function UserMenu({ user }: { user: UserMenuTarget }) {
  const close = useTrackMenuStore((s) => s.close);
  const navigate = useNavigate();

  const copyLink = async () => {
    const url = user.permalink;
    close();
    if (!url) return;
    try {
      await navigator.clipboard.writeText(url);
      toast.success('Copied!');
    } catch {
      toast.error('Something went wrong');
    }
  };

  return (
    <MenuShell>
      <MenuItem
        icon={<User size={15} />}
        label={'Go to profile'}
        onClick={() => {
          close();
          navigate(user.target);
        }}
      />
      {user.permalink && (
        <MenuItem icon={<LinkIcon size={15} />} label={'Copy link'} onClick={copyLink} />
      )}
    </MenuShell>
  );
}

/** App-wide right-click menu: track actions on rows/cards/now-playing, profile
 *  actions on artist links. Mounted once in the shell. */
export function TrackContextMenuHost() {
  const kind = useTrackMenuStore((s) => s.kind);
  const track = useTrackMenuStore((s) => s.track);
  const user = useTrackMenuStore((s) => s.user);
  // The playlist dialog outlives the menu (the menu closes when it opens).
  const [dialogTrack, setDialogTrack] = useState<Track | null>(null);

  if (!kind && !dialogTrack) return null;

  return createPortal(
    <>
      {kind === 'track' && track && (
        <TrackMenu track={track} onAddToPlaylist={() => setDialogTrack(track)} />
      )}
      {kind === 'user' && user && <UserMenu user={user} />}
      <AddToPlaylistDialog
        trackUrns={dialogTrack ? [dialogTrack.urn] : []}
        open={!!dialogTrack}
        onOpenChange={(v) => {
          if (!v) setDialogTrack(null);
        }}
      />
    </>,
    document.body,
  );
}
