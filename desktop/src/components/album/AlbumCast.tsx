import { memo, useMemo } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import type { Aura } from '../../lib/aura';
import { Users } from '../../lib/icons';
import { Avatar } from '../ui/Avatar';
import type { AlbumArtist } from './types';

interface AlbumCastProps {
  artists: AlbumArtist[];
  aura: Aura;
}

const ROLE_BUCKETS = ['primary', 'featured', 'remixer', 'producer'] as const;
const ROLE_LABEL_KEY: Record<string, string> = {
  primary: 'album.primaryArtist',
  featured: 'album.featured',
  remixer: 'album.remixer',
  producer: 'album.producer',
};

interface CastGroup {
  role: string;
  items: AlbumArtist[];
}

function groupByRole(artists: AlbumArtist[]): CastGroup[] {
  const map = new Map<string, AlbumArtist[]>();
  for (const a of artists) {
    const arr = map.get(a.role) ?? [];
    arr.push(a);
    map.set(a.role, arr);
  }
  const ordered: CastGroup[] = [];
  for (const k of ROLE_BUCKETS) {
    const items = map.get(k);
    if (items && items.length > 0) ordered.push({ role: k, items });
    map.delete(k);
  }
  for (const [k, items] of map) {
    if (items.length > 0) ordered.push({ role: k, items });
  }
  return ordered;
}

const CastCard = memo(function CastCard({
  artist,
  roleLabel,
}: {
  artist: AlbumArtist;
  roleLabel: string;
}) {
  const navigate = useNavigate();
  return (
    <button
      type="button"
      onClick={() => navigate(`/artist/${encodeURIComponent(artist.id)}`)}
      className="group flex cursor-pointer items-center gap-3 text-left"
    >
      <span className="relative h-12 w-12 shrink-0 overflow-hidden rounded-full ring-1 ring-white/10 transition-colors group-hover:ring-white/25">
        <Avatar src={artist.avatar_url} alt={artist.name} size={48} />
      </span>
      <span className="flex min-w-0 flex-col leading-tight">
        <span className="truncate text-[12px] font-semibold text-white/90 group-hover:text-white">
          {artist.name}
        </span>
        <span className="text-[10px] font-medium text-white/35">{roleLabel}</span>
      </span>
    </button>
  );
});

const CastRow = memo(function CastRow({ role, items }: { role: string; items: AlbumArtist[] }) {
  const { t } = useTranslation();
  const roleLabel = ROLE_LABEL_KEY[role] ? t(ROLE_LABEL_KEY[role]) : role;
  return (
    <div className="flex flex-col gap-3">
      <span className="text-[10px] font-medium text-white/30">
        {roleLabel} · {items.length}
      </span>
      <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6">
        {items.map((artist) => (
          <CastCard key={artist.id} artist={artist} roleLabel={roleLabel} />
        ))}
      </div>
    </div>
  );
});

function AlbumCastImpl({ artists }: AlbumCastProps) {
  const { t } = useTranslation();
  const groups = useMemo(() => groupByRole(artists), [artists]);

  if (artists.length === 0) return null;

  return (
    <section className="flex flex-col gap-4">
      <div className="flex items-center gap-2">
        <Users size={14} className="text-white/45" />
        <h3 className="text-[13px] font-medium text-white/70">
          {t('album.cast')} <span className="ml-1 text-white/30">{artists.length}</span>
        </h3>
      </div>
      <div className="flex flex-col gap-5">
        {groups.map((g) => (
          <CastRow key={g.role} role={g.role} items={g.items} />
        ))}
      </div>
    </section>
  );
}

export const AlbumCast = memo(AlbumCastImpl);
