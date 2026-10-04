import { memo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { type Aura, auraRgb } from '../../lib/aura';
import { fc } from '../../lib/formatters';
import { Check, ChevronDown, Globe, ListMusic, MicVocal, Music, Users } from '../../lib/icons';
import { AvatarArtifact } from '../user/AvatarArtifact';
import { VerifiedBadge } from '../user/UserChips';
import { SocialIcon, socialLabel } from './socials';
import type { ArtistDetail } from './types';

interface ArtistHeroProps {
  artist: ArtistDetail;
  hasStar: boolean;
  aura: Aura;
}

const SocialChip = memo(({ kind, url, title }: { kind: string; url: string; title: string }) => {
  return (
    <a
      href={url}
      target="_blank"
      rel="noreferrer"
      className="inline-flex items-center gap-1.5 text-[11px] font-semibold text-white/55 hover:text-white transition-colors"
    >
      <span className="text-white/45">
        <SocialIcon kind={kind} size={13} />
      </span>
      <span className="truncate max-w-[140px]">{title}</span>
    </a>
  );
});

const ScAccountChip = memo(
  ({ scUserId, role, verified }: { scUserId: string; role: string; verified: boolean }) => {
    const { t } = useTranslation();
    const navigate = useNavigate();
    const label =
      role === 'main' ? t('artist.mainAccount') : role === 'demo' ? t('artist.demoAccount') : role;
    return (
      <button
        type="button"
        onClick={() => navigate(`/user/${encodeURIComponent(`soundcloud:users:${scUserId}`)}`)}
        className="inline-flex items-center gap-1.5 text-[11px] font-semibold cursor-pointer transition-colors text-orange-200/85 hover:text-orange-100"
        title={role}
      >
        <SocialIcon kind="soundcloud" size={13} />
        <span className="truncate max-w-[120px]">{label}</span>
        {verified && <Check size={11} className="text-emerald-400 shrink-0" strokeWidth={3} />}
      </button>
    );
  },
);

function ArtistHeroImpl({ artist, hasStar, aura }: ArtistHeroProps) {
  const { t } = useTranslation();
  const [bioExpanded, setBioExpanded] = useState(false);

  return (
    <div className="flex flex-col">
      <div className="relative flex flex-col lg:flex-row gap-6 lg:gap-8 items-center lg:items-stretch">
        <AvatarArtifact
          username={artist.name}
          avatarUrl={artist.avatar_url}
          hasStar={hasStar}
          aura={aura}
        />

        <div className="flex-1 min-w-0 flex flex-col justify-start gap-5 text-center lg:text-left">
          {/* Top chips */}
          <div className="flex flex-wrap items-center gap-2 justify-center lg:justify-start">
            {artist.confidence >= 0.7 && (
              <VerifiedBadge
                title={t('track.verifiedArtist', { confidence: artist.confidence.toFixed(2) })}
              />
            )}
            {artist.country && (
              <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-white/55">
                <Globe size={11} className="text-white/45" /> {artist.country}
              </span>
            )}
            <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-white/55">
              <MicVocal size={11} className="text-white/45" /> {t('artist.title')}
            </span>
          </div>

          {/* Name */}
          <h1
            className="text-5xl md:text-7xl font-black leading-[0.85] tracking-tighter break-words max-w-full"
            style={{
              color: hasStar ? auraRgb(aura) : '#fff',
              textShadow: '0 8px 24px rgba(0,0,0,0.5)',
            }}
          >
            {artist.name}
          </h1>

          {/* Bio */}
          {artist.bio && (
            <button
              type="button"
              onClick={() => setBioExpanded((v) => !v)}
              className="group text-left cursor-pointer"
            >
              <p
                className={`selectable text-[14px] md:text-[15px] text-white/65 leading-relaxed max-w-2xl transition-all duration-700 ${
                  bioExpanded ? '' : 'line-clamp-2'
                }`}
              >
                {artist.bio}
              </p>
              <span className="inline-flex items-center gap-1 mt-1 text-[11px] font-semibold text-white/30 group-hover:text-white/60 transition-colors">
                <ChevronDown
                  size={12}
                  className={`transition-transform duration-500 ${bioExpanded ? 'rotate-180' : ''}`}
                />
                {bioExpanded ? t('common.collapse') : t('common.expand')}
              </span>
            </button>
          )}

          {/* Socials + SC accounts */}
          {(artist.socials.length > 0 || artist.sc_accounts.length > 0) && (
            <div className="flex flex-wrap gap-1.5 justify-center lg:justify-start lg:mt-auto lg:pt-2">
              {artist.sc_accounts.map((acc) => (
                <ScAccountChip
                  key={acc.sc_user_id}
                  scUserId={acc.sc_user_id}
                  role={acc.role}
                  verified={acc.verified}
                />
              ))}
              {artist.socials.map((s) => (
                <SocialChip key={s.url} kind={s.kind} url={s.url} title={socialLabel(s.kind)} />
              ))}
            </div>
          )}
        </div>

        {/* Right column stats */}
        <div className="hidden lg:flex flex-col gap-3 self-stretch min-w-[180px]">
          <HeroStat value={artist.track_count_primary} label={t('artist.statsTracks')} />
          <HeroStat value={artist.track_count_featured} label={t('artist.statsFeatured')} />
          <HeroStat value={artist.album_count} label={t('artist.statsAlbums')} />
          <HeroStat value={artist.related_artists.length} label={t('artist.statsRelated')} />
        </div>
      </div>

      {/* Stats strip on narrow */}
      <div className="lg:hidden flex flex-wrap gap-x-5 gap-y-2 mt-4 justify-center">
        <HeroStat
          icon={<Music size={12} />}
          value={artist.track_count_primary}
          label={t('artist.statsTracks')}
        />
        <HeroStat
          icon={<MicVocal size={12} />}
          value={artist.track_count_featured}
          label={t('artist.statsFeatured')}
        />
        <HeroStat
          icon={<ListMusic size={12} />}
          value={artist.album_count}
          label={t('artist.statsAlbums')}
        />
        <HeroStat
          icon={<Users size={12} />}
          value={artist.related_artists.length}
          label={t('artist.statsRelated')}
        />
      </div>
    </div>
  );
}

const HeroStat = memo(
  ({ icon, value, label }: { icon?: React.ReactNode; value: number; label: string }) => {
    return (
      <div className="inline-flex items-baseline gap-2">
        {icon && <span className="text-white/40">{icon}</span>}
        <span className="text-[15px] font-black tabular-nums text-white">{fc(value)}</span>
        <span className="text-[10px] font-medium text-white/35">{label}</span>
      </div>
    );
  },
);

export const ArtistHero = memo(ArtistHeroImpl);
