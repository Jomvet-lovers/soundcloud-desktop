import type { DragEndEvent } from '@dnd-kit/core';
import * as Dialog from '@radix-ui/react-dialog';
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { toast } from 'sonner';
import { useShallow } from 'zustand/shallow';
import { PLAYLIST_KEYFRAMES } from '../components/playlist/keyframes';
import { MoreCrates } from '../components/playlist/MoreCrates';
import { PlaylistHero } from '../components/playlist/PlaylistHero';
import { SequenceList } from '../components/playlist/SequenceList';
import {
  useDeletePlaylist,
  useInfiniteScroll,
  usePlaylist,
  usePlaylistTracks,
  useUpdatePlaylistTracks,
} from '../lib/hooks';
import { AlertCircle, X } from '../lib/icons';
import { rawPlaylistCover } from '../lib/playlist-cover';
import { armPlaylistContinuation } from '../lib/queue-continuation';
import { toastWriteError } from '../lib/write-error';
import { useAuthStore } from '../stores/auth';
import { pinPlaylist, unpinPlaylist, usePinnedPlaylists } from '../stores/pinned-playlists';
import { type Track, usePlayerStore } from '../stores/player';

function HeroSkeleton() {
  return (
    <div className="relative">
      <div className="flex flex-col lg:flex-row gap-10">
        <div className="w-[150px] h-[150px] md:w-[200px] md:h-[200px] rounded-[1.7rem] skeleton-shimmer shrink-0 self-center lg:self-start" />
        <div className="flex-1 space-y-4 w-full">
          <div className="h-4 w-32 rounded-full skeleton-shimmer" />
          <div className="h-14 w-2/3 rounded-2xl skeleton-shimmer" />
          <div className="h-11 w-72 rounded-full skeleton-shimmer mt-6" />
          <div className="h-24 w-full rounded-2xl skeleton-shimmer mt-4" />
        </div>
      </div>
    </div>
  );
}

export const PlaylistPage = React.memo(function PlaylistPage() {
  const { urn } = useParams<{ urn: string }>();
  const navigate = useNavigate();
  const myUrn = useAuthStore((s) => s.user?.urn);

  const {
    data: playlist,
    isLoading: playlistLoading,
    isError: playlistError,
    refetch: refetchPlaylist,
  } = usePlaylist(urn);
  const {
    tracks: playlistTracks,
    isLoading: tracksLoading,
    hasNextPage,
    isFetchingNextPage,
    fetchNextPage,
  } = usePlaylistTracks(urn);
  const updateTracks = useUpdatePlaylistTracks(urn);
  const deletePlaylist = useDeletePlaylist();
  const [showDeleteConfirm, setShowDeleteConfirm] = useState(false);

  const pinnedPlaylists = usePinnedPlaylists();

  const isLoading = playlistLoading || tracksLoading;
  const isOwner = !!playlist && !!myUrn && playlist.user?.urn === myUrn;
  const isPinned = pinnedPlaylists.some((item) => item.urn === playlist?.urn);

  const serverTracks: Track[] = useMemo(() => {
    if (isLoading || !playlist) return [];
    return playlistTracks.length > 0 ? playlistTracks : (playlist.tracks ?? []);
  }, [isLoading, playlist, playlistTracks]);

  // Local order for DnD; skip server sync while a debounced save is in flight.
  const [localTracks, setLocalTracks] = useState<Track[]>([]);
  // Rows are memoised on (urn, index), so a handler closing over `localTracks`
  // can go stale when another row is removed without shifting this one: the next
  // remove then filtered the old array and resurrected the deleted track. Read
  // the live list through refs instead.
  const localTracksRef = useRef<Track[]>([]);
  localTracksRef.current = localTracks;
  const serverTracksRef = useRef<Track[]>(serverTracks);
  serverTracksRef.current = serverTracks;
  const pendingMutationRef = useRef(false);
  useEffect(() => {
    if (!pendingMutationRef.current) setLocalTracks(serverTracks);
  }, [serverTracks]);

  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingSaveRef = useRef<{ urns: string[]; successMsg?: string } | null>(null);

  const sendSave = useCallback(
    (payload: { urns: string[]; successMsg?: string }) => {
      pendingSaveRef.current = null;
      updateTracks.mutate(payload.urns, {
        onSuccess: () => {
          pendingMutationRef.current = false;
          if (payload.successMsg) toast.success(payload.successMsg);
        },
        onError: (error) => {
          pendingMutationRef.current = false;
          setLocalTracks(serverTracksRef.current);
          // The rows snap back because SoundCloud refused the write; say so.
          toastWriteError(error, "Couldn't save the playlist order");
        },
      });
    },
    [updateTracks],
  );
  const sendSaveRef = useRef(sendSave);
  sendSaveRef.current = sendSave;

  const debouncedUpdate = useCallback((next: Track[], successMsg?: string) => {
    pendingMutationRef.current = true;
    const payload = { urns: next.map((tr) => tr.urn), successMsg };
    pendingSaveRef.current = payload;
    if (debounceTimerRef.current) clearTimeout(debounceTimerRef.current);
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      sendSaveRef.current(payload);
    }, 600);
  }, []);

  // Every route change unmounts this page (`AppShell` keys the outlet on the
  // pathname), so a pending save must be flushed on the way out: clearing the
  // timer only silently dropped the reorder/removal the user just saw.
  useEffect(
    () => () => {
      if (debounceTimerRef.current) {
        clearTimeout(debounceTimerRef.current);
        debounceTimerRef.current = null;
        const pending = pendingSaveRef.current;
        if (pending) sendSaveRef.current(pending);
      }
    },
    [],
  );

  const tracks = isOwner ? localTracks : serverTracks;

  const trackUrnSet = useMemo(() => new Set(tracks.map((tr) => tr.urn)), [tracks]);
  const { isPausedFromThis, isPlayingFromThis } = usePlayerStore(
    useShallow((s) => ({
      isPlayingFromThis:
        s.isPlaying && s.currentTrack != null && trackUrnSet.has(s.currentTrack.urn),
      isPausedFromThis:
        !s.isPlaying && s.currentTrack != null && trackUrnSet.has(s.currentTrack.urn),
    })),
  );

  const scrollRef = useInfiniteScroll(hasNextPage ?? false, isFetchingNextPage, fetchNextPage);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const current = localTracksRef.current;
      const oldIndex = current.findIndex((tr) => tr.urn === active.id);
      const newIndex = current.findIndex((tr) => tr.urn === over.id);
      if (oldIndex === -1 || newIndex === -1) return;
      const next = [...current];
      const [moved] = next.splice(oldIndex, 1);
      next.splice(newIndex, 0, moved);
      setLocalTracks(next);
      debouncedUpdate(next, 'Track order saved');
    },
    [debouncedUpdate],
  );

  const handleRemoveTrack = useCallback(
    (trackUrn: string) => {
      const next = localTracksRef.current.filter((tr) => tr.urn !== trackUrn);
      setLocalTracks(next);
      debouncedUpdate(next, 'Track removed');
    },
    [debouncedUpdate],
  );

  // Доигрываем плейлист ДО КОНЦА (пагинированный срез в очереди → потом волна),
  // как у лайков. Армить ПОСЛЕ play(): он сбрасывает прошлый источник.
  const armContinuation = useCallback(() => {
    if (urn) armPlaylistContinuation(urn);
  }, [urn]);

  const handlePlayAll = useCallback(() => {
    if (tracks.length === 0) return;
    const { play, pause, resume } = usePlayerStore.getState();
    if (isPlayingFromThis) pause();
    else if (isPausedFromThis) resume();
    else {
      play(tracks[0], tracks);
      armContinuation();
    }
  }, [tracks, isPlayingFromThis, isPausedFromThis, armContinuation]);

  const handleShuffle = useCallback(() => {
    if (tracks.length === 0) return;
    const st = usePlayerStore.getState();
    if (!st.shuffle) usePlayerStore.setState({ shuffle: true });
    st.play(tracks[Math.floor(Math.random() * tracks.length)], tracks);
    armContinuation();
  }, [tracks, armContinuation]);

  const handleTogglePin = useCallback(() => {
    if (!playlist) return;
    if (isPinned) {
      unpinPlaylist(playlist.urn);
      toast.success('Removed from sidebar');
      return;
    }
    pinPlaylist({
      urn: playlist.urn,
      title: playlist.title,
      artworkUrl: rawPlaylistCover(playlist.artwork_url, tracks, playlist.user?.avatar_url),
    });
    toast.success('Pinned to sidebar');
  }, [playlist, isPinned, tracks]);

  const handleDelete = useCallback(() => {
    if (!playlist) return;
    deletePlaylist.mutate(playlist.urn, {
      onSuccess: () => {
        toast.success('Playlist deleted');
        navigate(-1);
      },
    });
  }, [playlist, deletePlaylist, navigate]);

  if (isLoading) {
    return (
      <div className="relative min-h-full w-full">
        <style>{PLAYLIST_KEYFRAMES}</style>
        <div
          className="relative z-10 max-w-[1320px] mx-auto px-4 md:px-8 pt-5 pb-10"
          style={{ isolation: 'isolate' }}
        >
          <HeroSkeleton />
        </div>
      </div>
    );
  }

  // A deleted/private playlist used to keep the skeleton forever: checking
  // `!playlist` alone leaves no way out once the fetch has settled.
  if (!playlist) {
    return (
      <div className="relative w-full min-h-[60vh] flex flex-col items-center justify-center gap-4 px-6 text-center">
        <AlertCircle size={26} className="text-white/25" strokeWidth={1.7} />
        <div className="space-y-1">
          <p className="text-[14px] text-white/70">{'This playlist could not be loaded'}</p>
          <p className="text-[12px] text-white/35">
            {playlistError ? 'It may be private or deleted.' : 'Nothing came back.'}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void refetchPlaylist()}
            className="h-8 px-3.5 rounded-lg text-[12px] font-medium bg-white/[0.06] text-white/80 hover:bg-white/[0.1] transition-colors cursor-pointer"
          >
            {'Try again'}
          </button>
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="h-8 px-3.5 rounded-lg text-[12px] text-white/45 hover:text-white/75 hover:bg-white/[0.06] transition-colors cursor-pointer"
          >
            {'Go back'}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="relative min-h-full w-full">
      <style>{PLAYLIST_KEYFRAMES}</style>

      <div
        className="relative z-10 max-w-[1320px] mx-auto px-4 md:px-8 pt-6 pb-10 space-y-6 animate-soft-in"
        style={{ isolation: 'isolate' }}
      >
        <PlaylistHero
          playlist={playlist}
          isOwner={isOwner}
          isPlaying={isPlayingFromThis}
          isPinned={isPinned}
          onPlayAll={handlePlayAll}
          onShuffle={handleShuffle}
          onTogglePin={handleTogglePin}
          onDelete={() => setShowDeleteConfirm(true)}
        />

        <SequenceList
          tracks={tracks}
          isOwner={isOwner}
          onDragEnd={handleDragEnd}
          onRemove={handleRemoveTrack}
          onPlay={armContinuation}
          sentinelRef={scrollRef}
          hasNextPage={hasNextPage ?? false}
          isFetchingNextPage={isFetchingNextPage}
        />

        {playlist.user?.urn && (
          <MoreCrates
            curatorUrn={playlist.user.urn}
            curatorName={playlist.user.username}
            excludeUrn={playlist.urn}
          />
        )}
      </div>

      <Dialog.Root open={showDeleteConfirm} onOpenChange={setShowDeleteConfirm}>
        <Dialog.Portal>
          <Dialog.Overlay className="fixed inset-0 bg-black/60 z-50 animate-fade-in" />
          <Dialog.Content className="fixed top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 z-50 w-[380px] rounded-2xl bg-[#141417] border border-white/[0.08] shadow-2xl animate-fade-in-up p-6 space-y-4">
            <div className="flex items-center gap-3">
              <div className="w-10 h-10 rounded-full bg-red-500/10 flex items-center justify-center shrink-0">
                <AlertCircle size={20} className="text-red-400" />
              </div>
              <Dialog.Title className="text-[15px] font-bold text-white/90">
                {'Delete playlist'}
              </Dialog.Title>
              <Dialog.Close className="ml-auto w-7 h-7 rounded-lg flex items-center justify-center text-white/30 hover:text-white/70 hover:bg-white/[0.08] transition-all">
                <X size={14} />
              </Dialog.Close>
            </div>
            <p className="text-[13px] text-white/50 leading-relaxed">
              {`Delete "${playlist.title}"? This cannot be undone.`}
            </p>
            <div className="flex items-center justify-end gap-2.5 pt-1">
              <Dialog.Close className="px-4 py-2 rounded-xl text-[13px] font-medium text-white/50 hover:text-white/80 hover:bg-white/[0.06] transition-all cursor-pointer">
                {'Cancel'}
              </Dialog.Close>
              <button
                type="button"
                onClick={handleDelete}
                disabled={deletePlaylist.isPending}
                className="px-4 py-2 rounded-xl text-[13px] font-semibold bg-red-500/15 text-red-400 hover:bg-red-500/25 border border-red-500/20 transition-all cursor-pointer disabled:opacity-50"
              >
                {deletePlaylist.isPending ? 'Loading...' : 'Delete playlist'}
              </button>
            </div>
          </Dialog.Content>
        </Dialog.Portal>
      </Dialog.Root>
    </div>
  );
});
