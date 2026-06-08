import React, {useMemo, useState} from 'react';
import {useTranslation} from 'react-i18next';
import {useNavigate} from 'react-router-dom';
import {WaveFrame} from '../components/home/WaveFrame';
import {WaveMasthead} from '../components/home/WaveMasthead';
import {useSoundprint} from '../components/library/useSoundprint';
import {SoundWaveBlock, SoundWaveLockOverlay} from '../components/music/soundwave';
import {TrackCard} from '../components/music/TrackCard';
import {HorizontalScroll} from '../components/ui/HorizontalScroll';
import {Skeleton} from '../components/ui/Skeleton';
import {useDiscoverFeed, useLikedTracks,} from '../lib/hooks';
import {ChevronRight, Heart, Sparkles} from '../lib/icons';
import {usePerfMode} from '../lib/perf';
import {armLikesContinuation} from '../lib/queue-continuation';
import {useScdMeta} from '../lib/scdMeta';
import {useAuthStore} from '../stores/auth';
import type {Track} from '../stores/player';

/* ── Helpers ──────────────────────────────────────────────── */

// Retained-card cap per horizontal shelf in reduced perf modes; full set lives
// behind "see all". Beauty keeps every card (byte-identical to today).
const SHELF_CAP = 24;

function useShelfCap(): number {
    return usePerfMode().mode === 'beauty' ? Number.POSITIVE_INFINITY : SHELF_CAP;
}

/* ── Section Header ───────────────────────────────────────── */

function SectionHeader({
  title,
  icon,
  onSeeAll,
}: {
  title: string;
  icon: React.ReactNode;
  onSeeAll?: () => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="flex items-center justify-between mb-5">
      <div className="flex items-center gap-2.5">
        <div className="w-8 h-8 rounded-xl bg-white/[0.04] border border-white/[0.06] flex items-center justify-center">
          {icon}
        </div>
        <h2 className="text-[15px] font-semibold tracking-tight text-white/90">{title}</h2>
      </div>
      {onSeeAll && (
        <button
          type="button"
          onClick={onSeeAll}
          className="flex items-center gap-1 text-[11px] text-white/30 hover:text-white/60 transition-colors duration-200 cursor-pointer"
        >
          {t('common.seeAll')}
          <ChevronRight size={12} />
        </button>
      )}
    </div>
  );
}

/* ── Skeletons ────────────────────────────────────────────── */

function ShelfSkeleton({ count = 8 }: { count?: number }) {
  return (
    <>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="w-[180px] shrink-0">
          <Skeleton className="aspect-square w-full" rounded="lg" />
          <Skeleton className="h-4 w-3/4 mt-2.5" rounded="sm" />
          <Skeleton className="h-3 w-1/2 mt-1.5" rounded="sm" />
        </div>
      ))}
    </>
  );
}

/* ── Isolated Sections ────────────────────────────────────── */

const LikedShelf = React.memo(function LikedShelf({
  likedTracks,
  isLoading,
}: {
  likedTracks: Track[];
  isLoading: boolean;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
    const shelfCap = useShelfCap();

  if (!isLoading && likedTracks.length === 0) return null;

  return (
    <section>
      <SectionHeader
        title={t('library.likedTracks')}
        icon={<Heart size={15} className="text-accent" />}
        onSeeAll={() => navigate('/library')}
      />
      <HorizontalScroll>
        {isLoading ? (
          <ShelfSkeleton />
        ) : (
            likedTracks.slice(0, shelfCap).map((track) => (
            <div key={track.urn} className="w-[180px] shrink-0">
                <TrackCard track={track} queue={likedTracks} onPlay={armLikesContinuation}/>
            </div>
          ))
        )}
      </HorizontalScroll>
    </section>
  );
});

const RecommendedSection = React.memo(function RecommendedSection() {
  const { t } = useTranslation();
    const shelfCap = useShelfCap();
    const {recommended, isLoading} = useDiscoverFeed();
    const recommendedTracks = useScdMeta(recommended);

    if (!isLoading && recommendedTracks.length === 0) return null;

  return (
      <section>
          <SectionHeader
              title={t('home.recommended', 'Recommended For You')}
              icon={<Sparkles size={15} className="text-amber-400/70"/>}
          />
          <HorizontalScroll>
              {isLoading ? (
                  <ShelfSkeleton/>
              ) : (
                  recommendedTracks.slice(0, shelfCap).map((track) => (
                      <div key={track.urn} className="w-[180px] shrink-0">
                          <TrackCard track={track} queue={recommendedTracks}/>
                      </div>
                  ))
              )}
          </HorizontalScroll>
      </section>
  );
});

/* ── Home Page ────────────────────────────────────────────── */

export function Home() {
  const user = useAuthStore((s) => s.user);
  const likedTracksQuery = useLikedTracks(100);

    // Picked genre tag — retints the whole room (atmosphere, stars, masthead).
    const [genre, setGenre] = useState<string | null>(null);
    const sound = useSoundprint(likedTracksQuery.tracks, genre);

  const likedShelfTracks = useMemo(
    () => likedTracksQuery.tracks.slice(0, 50),
    [likedTracksQuery.tracks],
  );

  return (
      <WaveFrame sound={sound}>
          {user && (
              <WaveMasthead
                  user={user}
                  likedTracks={likedTracksQuery.tracks}
                  sound={sound}
                  selected={genre}
                  onSelect={setGenre}
              />
          )}

          {/* SoundWave — AI rivers (vibe-search surfaced in the masthead instead) */}
      <div className="relative">
          <SoundWaveBlock hideVibePortal/>
        <SoundWaveLockOverlay />
      </div>

          {/* Manual rivers under the wave — each isolated (own hooks, own boundary) */}
      <LikedShelf likedTracks={likedShelfTracks} isLoading={likedTracksQuery.isLoading} />
          <RecommendedSection/>
      </WaveFrame>
  );
}
