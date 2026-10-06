import React, { useState } from 'react';
import { useParams } from 'react-router-dom';
import { TrackCard } from '../components/music/TrackCard';
import { Pager } from '../components/ui/Pager';
import { type TagSort, useTagTracksPage } from '../lib/hooks';
import { Loader2 } from '../lib/icons';
import { withViewTransition } from '../lib/view-transition';

const SORTS: Array<{ id: TagSort; label: string }> = [
  { id: 'newest', label: 'Newest' },
  { id: 'plays', label: 'Plays' },
  { id: 'likes', label: 'Likes' },
];

/** Tracks carrying a SoundCloud tag — the tag chips on a track page (and
 *  `#tag` in the search box) lead here. Numbered paging + sorting like the
 *  search page. */
export const TagPage = React.memo(function TagPage() {
  const { tag } = useParams<{ tag: string }>();
  const label = tag ?? '';

  const [sort, setSort] = useState<TagSort>('newest');
  const [pager, setPager] = useState<{ key: string; page: number }>({ key: '', page: 0 });
  const pagerKey = `${label}\u{1}${sort}`;
  const page = pager.key === pagerKey ? pager.page : 0;
  const changePage = (next: number) => {
    setPager({ key: pagerKey, page: Math.max(0, next) });
    (document.querySelector('main') as HTMLElement | null)?.scrollTo({
      top: 0,
      behavior: 'smooth',
    });
  };

  const query = useTagTracksPage(label || undefined, page, sort);

  return (
    <div className="px-5 py-6 md:px-8">
      <h1 className="text-[24px] font-semibold tracking-tight text-white/92">{`#${label}`}</h1>

      <div className="mt-4 flex flex-wrap items-center gap-1">
        {SORTS.map((opt) => {
          const on = sort === opt.id;
          return (
            <button
              key={opt.id}
              type="button"
              onClick={() => withViewTransition(() => setSort(opt.id))}
              className={`relative rounded-lg px-2.5 py-1.5 text-[11px] font-medium transition-colors cursor-pointer ${
                on ? 'text-white/90' : 'text-white/40 hover:text-white/70'
              }`}
            >
              <span
                aria-hidden
                className={`pointer-events-none absolute inset-0 rounded-lg bg-white/[0.1] transition-opacity duration-200 ${
                  on ? 'opacity-100' : 'opacity-0'
                }`}
              />
              <span className="relative">{opt.label}</span>
            </button>
          );
        })}
      </div>

      {query.isLoading ? (
        <div className="flex justify-center py-16">
          <Loader2 size={20} className="text-white/15 animate-spin" />
        </div>
      ) : query.tracks.length === 0 ? (
        <p className="mt-8 text-[13px] text-white/35">{'No tracks found'}</p>
      ) : (
        <>
          <div className="mt-5 grid grid-cols-3 gap-2.5 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8">
            {query.tracks.map((track) => (
              <TrackCard key={track.urn} track={track} queue={query.tracks} />
            ))}
          </div>
          <Pager
            page={page}
            hasMore={query.hasMore}
            isFetching={query.isFetching}
            onPage={changePage}
          />
        </>
      )}
    </div>
  );
});
