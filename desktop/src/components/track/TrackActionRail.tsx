import React from 'react';
import { ListPlus } from '../../lib/icons';
import type { Track } from '../../stores/player';
import { AddToPlaylistDialog } from '../music/AddToPlaylistDialog';
import { SharingToggle } from '../music/SharingToggle';
import { CopyIconAction, DownloadButton, LikeBtn } from './actions';

/** Engagement + utility row under the wave: the like chip, whatever the
 *  caller slots in the middle (the comment composer), then the icon group. */
export const TrackActionRail = React.memo(function TrackActionRail({
  track,
  isOwner,
  children,
}: {
  track: Track;
  isOwner: boolean;
  children?: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-3 flex-wrap">
      <LikeBtn trackUrn={track.urn} count={track.favoritings_count ?? track.likes_count} />
      {children}
      <div className="flex items-center gap-0.5">
        <AddToPlaylistDialog trackUrns={[track.urn]}>
          <button
            type="button"
            title={'Add to playlist'}
            aria-label={'Add to playlist'}
            className="inline-flex items-center justify-center w-10 h-10 rounded-full border border-white/[0.14] text-white/60 hover:text-white hover:border-white/[0.32] transition-colors cursor-pointer"
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
