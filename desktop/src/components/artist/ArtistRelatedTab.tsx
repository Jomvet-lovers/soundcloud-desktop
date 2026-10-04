import { memo } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { type Aura, auraRgb } from '../../lib/aura';
import { Globe, Users } from '../../lib/icons';
import { Avatar } from '../ui/Avatar';
import type { RelatedArtist } from './types';

interface ArtistRelatedTabProps {
  related: RelatedArtist[];
  aura: Aura;
}

function ArtistRelatedTabImpl({ related, aura }: ArtistRelatedTabProps) {
  const { t } = useTranslation();
  if (related.length === 0) {
    return (
      <div className="py-24 flex flex-col items-center gap-4">
        <Users size={24} className="text-white/15" />
        <p className="text-white/30 text-sm">{t('artist.noRelated')}</p>
      </div>
    );
  }

  const max = Math.max(...related.map((r) => r.weight), 1);

  return (
    <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 lg:grid-cols-6 xl:grid-cols-8 gap-4">
      {related.map((a) => (
        <RelatedCard key={a.id} item={a} aura={aura} maxWeight={max} />
      ))}
    </div>
  );
}

const RelatedCard = memo(
  ({ item, aura, maxWeight }: { item: RelatedArtist; aura: Aura; maxWeight: number }) => {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const pct = Math.max(0.08, Math.min(1, item.weight / maxWeight));
    return (
      <button
        type="button"
        onClick={() => navigate(`/artist/${encodeURIComponent(item.id)}`)}
        className="group relative flex flex-col items-center gap-3 cursor-pointer transition-all duration-500 hover:scale-[1.03]"
      >
        <div className="relative w-20 h-20 rounded-full overflow-hidden ring-2 ring-white/10 group-hover:ring-white/30 transition-all duration-500">
          <Avatar src={item.avatar_url} alt={item.name} size={80} />
        </div>
        <div className="text-center min-w-0 w-full relative">
          <p className="text-[13px] font-semibold text-white/90 truncate">{item.name}</p>
          {item.country && (
            <p className="inline-flex items-center gap-1 text-[10px] text-white/35 mt-0.5">
              <Globe size={9} /> {item.country}
            </p>
          )}
        </div>
        <div className="relative w-full h-1 rounded-full overflow-hidden bg-white/[0.04]">
          <div
            className="absolute inset-y-0 left-0 rounded-full"
            style={{
              width: `${pct * 100}%`,
              background: auraRgb(aura),
            }}
          />
        </div>
        <span className="text-[9px] font-medium text-white/30">
          {t('artist.affinity')} {(pct * 100).toFixed(0)}%
        </span>
      </button>
    );
  },
);

export const ArtistRelatedTab = memo(ArtistRelatedTabImpl);
