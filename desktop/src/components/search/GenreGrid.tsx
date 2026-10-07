import { memo } from 'react';
import { useNavigate } from 'react-router-dom';
import { art } from '../../lib/formatters';
import { useDiscoverMixed, useTagTracksPage } from '../../lib/hooks';
import { GENRES, genreColor } from './utils';

interface GenreCard {
  name: string;
  artwork: string | null;
}

/** Widest breakpoint is lg:grid-cols-4 — the wall is padded to a multiple of
 *  this so the last row never leaves a hole. */
const GRID_COLS = 4;

/** SC's "Trending by genre" collection is usually one short of a full row;
 *  top it up with the hand-tuned GENRES list, skipping names already shown. */
function fillToGridRow(list: GenreCard[]): GenreCard[] {
  if (list.length === 0 || list.length % GRID_COLS === 0) return list;
  const seen = new Set(list.map((c) => c.name.toLowerCase()));
  const out = [...list];
  for (const g of GENRES) {
    if (out.length % GRID_COLS === 0) break;
    if (seen.has(g.label.toLowerCase())) continue;
    seen.add(g.label.toLowerCase());
    out.push({ name: g.label, artwork: null });
  }
  return out;
}

const GenreTile = memo(function GenreTile({ card }: { card: GenreCard }) {
  const navigate = useNavigate();
  // Filler tiles have no artwork from SC's mixes — borrow the genre's first
  // track art so the wall stays visually even.
  const needsArt = !card.artwork;
  const tagQuery = useTagTracksPage(needsArt ? card.name.toLowerCase() : undefined, 0);
  const artwork = card.artwork ?? tagQuery.tracks.find((t) => t.artwork_url)?.artwork_url ?? null;

  return (
    <button
      type="button"
      onClick={() => navigate(`/tag/${encodeURIComponent(card.name)}`)}
      className="group relative aspect-[16/9] overflow-hidden rounded-xl p-3 text-left cursor-pointer transition-transform duration-300 ease-[var(--ease-apple)] hover:scale-[1.02] animate-soft-in"
      style={{ background: genreColor(card.name) }}
    >
      <span className="relative z-10 block max-w-[72%] text-[15px] font-black leading-tight tracking-tight text-black/85">
        {card.name}
      </span>
      {artwork && (
        <img
          src={art(artwork, 't200x200') ?? ''}
          alt=""
          loading="lazy"
          className="absolute -bottom-2 -right-2 h-[74%] w-auto rotate-[25deg] rounded-md object-cover shadow-[0_8px_20px_rgba(0,0,0,0.4)] transition-transform duration-300 ease-[var(--ease-apple)] group-hover:rotate-[18deg]"
        />
      )}
    </button>
  );
});

/** "Browse all genres" — Spotify-style tiles. SC's "Trending by genre" mixes
 *  carry the artwork; without them the hand-tuned GENRES list stands in. Each
 *  tile opens the genre's tag feed. */
export function GenreGrid() {
  const mixed = useDiscoverMixed();

  const trending = (mixed.data?.collection ?? []).find((sel) => /genre/i.test(sel.title));
  const items = (trending?.items?.collection ?? []).filter(
    (it) => !/^all genres$/i.test((it.short_title || it.title).trim()),
  );

  const cards: GenreCard[] = fillToGridRow(
    items.length > 0
      ? items.map((it) => ({
          name: it.short_title || it.title,
          artwork: it.artwork_url ?? it.calculated_artwork_url ?? null,
        }))
      : GENRES.map((g) => ({ name: g.label, artwork: null })),
  );

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
        <GenreTile key={c.name} card={c} />
      ))}
    </div>
  );
}
