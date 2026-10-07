import { Link } from 'react-router-dom';
import { TrackCard } from '../components/music/TrackCard';
import { greeting } from '../lib/greeting';
import { useLikedTracks } from '../lib/hooks';
import { ChevronRight } from '../lib/icons';
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
        {user ? greeting(user.username) : 'Home'}
      </h1>

      <div className="mt-5 flex items-center justify-between gap-4">
        <h2 className="text-[16px] font-semibold tracking-tight text-white/90">{'Liked Tracks'}</h2>
        {tracks.length > 0 && (
          <Link
            to="/library/likes"
            className="flex items-center gap-0.5 text-[12px] font-semibold text-white/45 hover:text-white/90 transition-colors"
          >
            {'See all'}
            <ChevronRight size={14} />
          </Link>
        )}
      </div>

      {tracks.length === 0 ? (
        <p className="mt-4 text-[13px] text-white/35">{'No liked tracks yet'}</p>
      ) : (
        <div className="mt-3 grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-7">
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
