import { memo, useMemo } from 'react';
import { useNavigate } from 'react-router-dom';
import type { Aura } from '../../lib/aura';
import { dur } from '../../lib/formatters';
import { Calendar, Disc3, ListMusic, MicVocal } from '../../lib/icons';
import { Avatar } from '../ui/Avatar';
import { AlbumCoverArtifact } from './AlbumCoverArtifact';
import { AlbumPlayButton } from './AlbumPlayButton';
import type { AlbumArtist, AlbumDetail } from './types';

interface AlbumHeroProps {
  album: AlbumDetail;
  hasStar: boolean;
  aura: Aura;
}

const ROLE_LABEL_KEY: Record<string, string> = {
  primary: 'Primary',
  featured: 'Featured',
  remixer: 'Remixer',
  producer: 'Producer',
};

const KIND_LABEL: Record<string, string> = {
  album: 'Album',
  ep: 'EP',
  single: 'Single',
  compilation: 'Compilation',
};

const ArtistChip = memo(function ArtistChip({
  id,
  name,
  role,
  avatarUrl,
}: {
  id: string;
  name: string;
  role: string;
  avatarUrl?: string;
}) {
  const navigate = useNavigate();
  const subLabel = ROLE_LABEL_KEY[role] ?? role;
  return (
    <button
      type="button"
      onClick={() => navigate(`/artist/${encodeURIComponent(id)}`)}
      className="group inline-flex items-center gap-2 cursor-pointer"
    >
      <span className="relative h-7 w-7 overflow-hidden rounded-full ring-1 ring-white/15">
        <Avatar src={avatarUrl} alt={name} size={28} />
      </span>
      <span className="flex flex-col items-start leading-tight">
        <span className="text-[12px] font-semibold text-white/90 group-hover:text-white">
          {name}
        </span>
        <span className="text-[10px] font-medium text-white/35">{subLabel}</span>
      </span>
    </button>
  );
});

function AlbumHeroImpl({ album, hasStar, aura }: AlbumHeroProps) {
  const kind = (album.type ?? 'album').toLowerCase();
  const kindLabel = KIND_LABEL[kind] ?? kind;

  const { totalDuration, indexedCount, featured } = useMemo(() => {
    let total = 0;
    let indexed = 0;
    const feat: AlbumArtist[] = [];
    for (const tr of album.tracks ?? []) {
      total += tr.duration ?? 0;
      if (tr.enrichment?.availability !== 'wanted') indexed++;
    }
    for (const a of album.artists ?? []) if (a.role !== 'primary') feat.push(a);
    return { totalDuration: total, indexedCount: indexed, featured: feat };
  }, [album.tracks, album.artists]);

  return (
    <div className="flex flex-col items-center gap-5 md:flex-row md:items-center md:gap-6">
      <AlbumCoverArtifact
        title={album.title}
        coverUrl={album.cover_url}
        hasStar={hasStar}
        aura={aura}
      />

      <div className="flex w-full min-w-0 flex-1 flex-col gap-3 text-center md:text-left">
        <h1 className="max-w-full break-words text-3xl font-black leading-tight tracking-tight text-white md:text-5xl">
          {album.title}
        </h1>

        {(album.primary_artist || featured.length > 0) && (
          <div className="flex flex-wrap items-center justify-center gap-3 md:justify-start">
            {album.primary_artist && (
              <ArtistChip
                id={album.primary_artist.id}
                name={album.primary_artist.name}
                role="primary"
                avatarUrl={album.primary_artist.avatar_url}
              />
            )}
            {featured.map((a) => (
              <ArtistChip
                key={a.id}
                id={a.id}
                name={a.name}
                role={a.role}
                avatarUrl={a.avatar_url}
              />
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-[11px] text-white/45 md:justify-start">
          <span className="inline-flex items-center gap-1.5">
            <Disc3 size={11} /> {kindLabel}
          </span>
          {album.release_year && (
            <span className="inline-flex items-center gap-1.5">
              <Calendar size={11} /> {album.release_year}
            </span>
          )}
          <span className="inline-flex items-center gap-1.5">
            <ListMusic size={11} />{' '}
            {`${album.tracks?.length ?? 0} ${(album.tracks?.length ?? 0) === 1 ? 'track' : 'tracks'}`}
          </span>
          {totalDuration > 0 && (
            <span className="inline-flex items-center gap-1.5">
              <MicVocal size={11} /> {dur(totalDuration)}
            </span>
          )}
          {indexedCount < (album.tracks?.length ?? 0) && (
            <span>{`${indexedCount}/${album.tracks?.length ?? 0} available`}</span>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-center gap-3 pt-1 md:justify-start">
          <AlbumPlayButton tracks={album.tracks ?? []} aura={aura} />
        </div>
      </div>
    </div>
  );
}

export const AlbumHero = memo(AlbumHeroImpl);
