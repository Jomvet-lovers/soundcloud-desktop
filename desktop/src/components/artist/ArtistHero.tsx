import { memo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { type Aura, auraRgba } from '../../lib/aura';
import { Check, ChevronDown, Globe, ListMusic, MicVocal, Music, Users } from '../../lib/icons';
import type { Track } from '../../stores/player';
import { GlassHeroPanel } from '../ui/GlassHeroPanel';
import { AvatarArtifact } from '../user/AvatarArtifact';
import { StatOrb } from '../user/StatOrb';
import { InfoChip, VerifiedBadge } from '../user/UserChips';
import { ArtistMixButton } from './ArtistMixButton';
import { SocialIcon, socialLabel } from './socials';
import type { ArtistDetail } from './types';

interface ArtistHeroProps {
  artist: ArtistDetail;
  hasStar: boolean;
  aura: Aura;
  popularTracks: Track[];
}

const SocialChip = memo(({ kind, url, title }: { kind: string; url: string; title: string }) => (
  <a
    href={url}
    target="_blank"
    rel="noreferrer"
    className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-[11px] font-semibold text-white/55 hover:text-white transition-all duration-300 hover:scale-105"
    style={{
      background: 'rgba(255,255,255,0.04)',
      border: '0.5px solid rgba(255,255,255,0.08)',
      backdropFilter: 'blur(16px)',
      WebkitBackdropFilter: 'blur(16px)',
    }}
  >
    <span className="text-white/45 group-hover:text-white">
      <SocialIcon kind={kind} size={13} />
    </span>
    <span className="truncate max-w-[140px]">{title}</span>
  </a>
));

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
        className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full text-[11px] font-semibold cursor-pointer transition-all duration-300 hover:scale-105 text-orange-200/85 hover:text-orange-100"
        style={{
          background: 'linear-gradient(135deg, rgba(255,85,0,0.16), rgba(255,0,128,0.06))',
          border: '0.5px solid rgba(255,85,0,0.25)',
          backdropFilter: 'blur(16px)',
          WebkitBackdropFilter: 'blur(16px)',
        }}
        title={role}
      >
        <SocialIcon kind="soundcloud" size={13} />
        <span className="truncate max-w-[120px]">{label}</span>
        {verified && <Check size={11} className="text-emerald-400 shrink-0" strokeWidth={3} />}
      </button>
    );
  },
);

function ArtistHeroImpl({ artist, hasStar, aura, popularTracks }: ArtistHeroProps) {
  const { t } = useTranslation();
  const [bioExpanded, setBioExpanded] = useState(false);
  const accent = auraRgba(aura, 0.18);

  return (
    <GlassHeroPanel hasStar={hasStar} aura={aura}>
      <div className="relative p-6 md:p-10 flex flex-col lg:flex-row gap-8 lg:gap-10 items-center lg:items-stretch">
        <AvatarArtifact
          username={artist.name}
          avatarUrl={artist.avatar_url}
          hasStar={hasStar}
          aura={aura}
        />

        <div className="flex-1 min-w-0 flex flex-col justify-between gap-5 text-center lg:text-left">
          {/* Top chips */}
          <div className="flex flex-wrap items-center gap-2 justify-center lg:justify-start">
            {artist.confidence >= 0.7 && (
              <VerifiedBadge
                title={t('track.verifiedArtist', { confidence: artist.confidence.toFixed(2) })}
              />
            )}
            {artist.country && <InfoChip icon={<Globe size={11} />}>{artist.country}</InfoChip>}
            <InfoChip icon={<MicVocal size={11} />}>{t('artist.title')}</InfoChip>
          </div>

          {/* Name */}
          <h1
            className="text-5xl md:text-7xl font-black leading-[0.85] tracking-tighter break-words max-w-full"
            style={
              hasStar
                ? {
                    background: aura.nameGradient,
                    backgroundSize: '200% auto',
                    WebkitBackgroundClip: 'text',
                    WebkitTextFillColor: 'transparent',
                    backgroundClip: 'text',
                    animation: 'prismatic-shift 6s linear infinite',
                    filter: 'drop-shadow(0 8px 24px rgba(0,0,0,0.5))',
                  }
                : { color: '#fff', textShadow: '0 8px 24px rgba(0,0,0,0.5)' }
            }
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
                className={`text-[14px] md:text-[15px] text-white/65 leading-relaxed max-w-2xl transition-all duration-700 ${
                  bioExpanded ? '' : 'line-clamp-2'
                }`}
              >
                {artist.bio}
              </p>
              <span className="inline-flex items-center gap-1 mt-1 text-[11px] font-semibold text-white/30 uppercase tracking-[0.18em] group-hover:text-white/60 transition-colors">
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
            <div className="flex flex-wrap gap-1.5 justify-center lg:justify-start">
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

          {/* Actions */}
          <div className="flex flex-wrap items-center gap-3 pt-1 justify-center lg:justify-start">
            <ArtistMixButton tracks={popularTracks} aura={aura} />
          </div>
        </div>

        {/* Right column stats */}
        <div className="hidden xl:flex flex-col gap-3 self-stretch min-w-[180px]">
          <StatOrb
            value={artist.track_count_primary}
            label={t('artist.statsTracks')}
            accent={accent}
          />
          <StatOrb
            value={artist.track_count_featured}
            label={t('artist.statsFeatured')}
            accent={accent}
          />
          <StatOrb value={artist.album_count} label={t('artist.statsAlbums')} accent={accent} />
          <StatOrb
            value={artist.related_artists.length}
            label={t('artist.statsRelated')}
            accent={accent}
          />
        </div>
      </div>

      {/* Stats strip on narrow */}
      <div className="xl:hidden flex flex-wrap gap-2 px-6 md:px-10 pb-6 md:pb-8 justify-center lg:justify-start">
        <CompactStat
          icon={<Music size={12} />}
          value={artist.track_count_primary}
          label={t('artist.statsTracks')}
        />
        <CompactStat
          icon={<MicVocal size={12} />}
          value={artist.track_count_featured}
          label={t('artist.statsFeatured')}
        />
        <CompactStat
          icon={<ListMusic size={12} />}
          value={artist.album_count}
          label={t('artist.statsAlbums')}
        />
        <CompactStat
          icon={<Users size={12} />}
          value={artist.related_artists.length}
          label={t('artist.statsRelated')}
        />
      </div>
    </GlassHeroPanel>
  );
}

const CompactStat = memo(
  ({ icon, value, label }: { icon: React.ReactNode; value: number; label: string }) => (
    <div
      className="inline-flex items-baseline gap-2 px-3.5 py-2 rounded-xl"
      style={{
        background: 'rgba(255,255,255,0.04)',
        border: '0.5px solid rgba(255,255,255,0.08)',
        backdropFilter: 'blur(20px)',
        WebkitBackdropFilter: 'blur(20px)',
      }}
    >
      <span className="text-white/40">{icon}</span>
      <span className="text-[15px] font-black tabular-nums text-white">{value}</span>
      <span className="text-[10px] font-bold uppercase tracking-[0.18em] text-white/35">
        {label}
      </span>
    </div>
  ),
);

export const ArtistHero = memo(ArtistHeroImpl);
