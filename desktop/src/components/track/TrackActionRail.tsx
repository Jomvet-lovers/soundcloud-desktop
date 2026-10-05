import React from 'react';
import { ListPlus } from '../../lib/icons';
import type { Track } from '../../stores/player';
import { AddToPlaylistDialog } from '../music/AddToPlaylistDialog';
import { SharingToggle } from '../music/SharingToggle';
import { CopyIconAction, DownloadButton, LikeBtn, PlayPill } from './actions';

/** Hero transport + engagement + utility rail. Lives OUTSIDE the genre-scoped
 *  wave wrapper, so play/like keep the user's own accent. */
export const TrackActionRail = React.memo(function TrackActionRail({
  track,
  isPlaying,
  isOwner,
  onPlay,
}: {
  track: Track;
  isPlaying: boolean;
  isOwner: boolean;
  onPlay: () => void;
}) {
  return (
    <div className="flex items-center gap-3 flex-wrap">
      <PlayPill isPlaying={isPlaying} onClick={onPlay} />
      <LikeBtn trackUrn={track.urn} count={track.favoritings_count ?? track.likes_count} />
      <span className="w-px h-5 bg-white/[0.08] mx-1" aria-hidden />
      <div className="flex items-center gap-0.5">
        <AddToPlaylistDialog trackUrns={[track.urn]}>
          <button
            type="button"
            title={'Add to playlist'}
            aria-label={'Add to playlist'}
            className="inline-flex items-center justify-center w-10 h-10 rounded-xl text-white/60 hover:text-white/95 hover:bg-white/[0.07] transition-all duration-200 cursor-pointer"
          >
            <ListPlus size={16} />
          </button>
        </AddToPlaylistDialog>
        <CopyIconAction url={track.permalink_url} />
        <DownloadButton track={track} />
        {isOwner && <SharingToggle kind="track" urn={track.urn} sharing={track.sharing} />}
      </div>
    </div>
  );
});
