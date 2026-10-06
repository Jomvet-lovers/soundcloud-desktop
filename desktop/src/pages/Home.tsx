import { TrackCard } from '../components/music/TrackCard';
import { useLikedTracks } from '../lib/hooks';
import { useAuthStore } from '../stores/auth';
import { DiscoverSections } from './Search';

/** Home — the liked-tracks shelf plus the Discover rows. Search results live on
 *  their own tab (pages/Search.tsx). */
export function Home() {
  const user = useAuthStore((s) => s.user);
  const liked = useLikedTracks(60);
  const tracks = liked.tracks;

  return (
    <div className="px-5 py-6 md:px-8">
      <h1 className="text-[24px] font-semibold tracking-tight text-white/92">
        {user ? `Good afternoon, ${user.username}` : 'Home'}
      </h1>
      <p className="mt-1 text-[13px] text-white/45">{'Liked Tracks'}</p>

      {tracks.length === 0 ? (
        <p className="mt-6 text-[13px] text-white/35">{'No liked tracks yet'}</p>
      ) : (
        <div className="mt-5 grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-7">
          {tracks.map((track) => (
            <TrackCard key={track.urn} track={track} queue={tracks} />
          ))}
        </div>
      )}

      <div className="mt-10">
        <DiscoverSections />
      </div>
    </div>
  );
}
