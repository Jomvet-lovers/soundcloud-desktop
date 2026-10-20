import React, { useMemo } from 'react';
import { FreshDrops } from '../components/feed/FreshDrops';
import { ArtistMiniCard } from '../components/library/ArtistMiniCard';
import { CollectionRail } from '../components/library/CollectionRail';
import { ContinueRow } from '../components/library/ContinueRow';
import { LibraryFrame } from '../components/library/LibraryFrame';
import { SoundPrintMasthead } from '../components/library/SoundPrintMasthead';
import { PlaylistCard } from '../components/music/PlaylistCard';
import { TrackCard } from '../components/music/TrackCard';
import { useLikedTracks, useMyFollowings, useMyLikedPlaylists, useMyPlaylists } from '../lib/hooks';
import { Bookmark, Heart, ListMusic, Users } from '../lib/icons';
import { likedTracksCount } from '../lib/likes';
import { armLikesContinuation } from '../lib/queue-continuation';
import { useAuthStore } from '../stores/auth';

/** Library "Hub" — a living home base. Your identity up top, then the reason to
 *  come back (fresh drops from who you follow), then a way back into what you
 *  were playing, then shelves into the deep pages of your collection. */
export const Library = React.memo(() => {
  const user = useAuthStore((s) => s.user);
  const { tracks: likedTracks } = useLikedTracks();

  const { playlists } = useMyPlaylists();
  const { playlists: likedPlaylists } = useMyLikedPlaylists();
  const { users: followings } = useMyFollowings();

  const playlistPreview = useMemo(() => playlists.slice(0, 12), [playlists]);
  const likedPlaylistPreview = useMemo(() => likedPlaylists.slice(0, 12), [likedPlaylists]);
  const artistPreview = useMemo(() => followings.slice(0, 14), [followings]);
  const likesPreview = useMemo(() => likedTracks.slice(0, 12), [likedTracks]);

  if (!user) return null;

  return (
    <LibraryFrame>
      <div className="space-y-9">
        <SoundPrintMasthead user={user} likedTracks={likedTracks} />

        <FreshDrops />

        <ContinueRow />

        {playlistPreview.length > 0 && (
          <CollectionRail
            icon={<ListMusic size={16} />}
            title={'Your Playlists'}
            count={user.playlist_count}
            to="/library/playlists"
          >
            {playlistPreview.map((p) => (
              <div key={p.urn} className="w-[120px] shrink-0">
                <PlaylistCard playlist={p} />
              </div>
            ))}
          </CollectionRail>
        )}

        {likedPlaylistPreview.length > 0 && (
          <CollectionRail
            icon={<Bookmark size={16} />}
            title={'Liked Playlists'}
            to="/library/playlists"
          >
            {likedPlaylistPreview.map((p) => (
              <div key={p.urn} className="w-[120px] shrink-0">
                <PlaylistCard playlist={p} />
              </div>
            ))}
          </CollectionRail>
        )}

        {artistPreview.length > 0 && (
          <CollectionRail
            icon={<Users size={16} />}
            title={'Artists'}
            count={user.followings_count}
            to="/library/following"
          >
            {artistPreview.map((u) => (
              <ArtistMiniCard key={u.urn} user={u} />
            ))}
          </CollectionRail>
        )}

        {likesPreview.length > 0 && (
          <CollectionRail
            icon={<Heart size={16} />}
            title={'Liked Tracks'}
            count={likedTracksCount(user)}
            to="/library/likes"
          >
            {likesPreview.map((tr) => (
              <div key={tr.urn} className="w-[112px] shrink-0">
                <TrackCard track={tr} queue={likesPreview} onPlay={armLikesContinuation} />
              </div>
            ))}
          </CollectionRail>
        )}
      </div>
    </LibraryFrame>
  );
});
