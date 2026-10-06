import { useNavigate } from 'react-router-dom';
import { art } from '../../lib/formatters';
import { useDiscoverMixed } from '../../lib/hooks';
import { GENRES, genreColor } from './utils';

interface GenreCard {
  name: string;
  artwork: string | null;
}

/** "Browse all genres" — Spotify-style tiles. SC's "Trending by genre" mixes
 *  carry the artwork; without them the hand-tuned GENRES list stands in. Each
 *  tile opens the genre's tag feed. */
export function GenreGrid() {
  const navigate = useNavigate();
  const mixed = useDiscoverMixed();

  const trending = (mixed.data?.collection ?? []).find((sel) => /genre/i.test(sel.title));
  const items = (trending?.items?.collection ?? []).filter(
    (it) => !/^all genres$/i.test((it.short_title || it.title).trim()),
  );

  const cards: GenreCard[] =
    items.length > 0
      ? items.map((it) => ({
          name: it.short_title || it.title,
          artwork: it.artwork_url ?? it.calculated_artwork_url ?? null,
        }))
      : GENRES.map((g) => ({ name: g.label, artwork: null }));

  if (mixed.isLoading) {
    return (
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
        {Array.from({ length: 8 }, (_, i) => (
          <div key={i} className="aspect-[16/9] rounded-xl skeleton-shimmer" />
        ))}
      </div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
      {cards.map((c) => (
        <button
          key={c.name}
          type="button"
          onClick={() => navigate(`/tag/${encodeURIComponent(c.name)}`)}
          className="group relative aspect-[16/9] overflow-hidden rounded-xl p-3 text-left cursor-pointer transition-transform duration-300 ease-[var(--ease-apple)] hover:scale-[1.02]"
          style={{ background: genreColor(c.name) }}
        >
          <span className="relative z-10 block max-w-[72%] text-[15px] font-black leading-tight tracking-tight text-black/85">
            {c.name}
          </span>
          {c.artwork && (
            <img
              src={art(c.artwork, 't200x200') ?? ''}
              alt=""
              loading="lazy"
              className="absolute -bottom-2 -right-2 h-[74%] w-auto rotate-[25deg] rounded-md object-cover shadow-[0_8px_20px_rgba(0,0,0,0.4)] transition-transform duration-300 ease-[var(--ease-apple)] group-hover:rotate-[18deg]"
            />
          )}
        </button>
      ))}
    </div>
  );
}
