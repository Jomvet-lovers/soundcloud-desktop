import { useTranslation } from 'react-i18next';
import { type Aura, auraRgba } from '../../lib/aura';
import { Calendar, Globe } from '../../lib/icons';
import { likedTracksCount } from '../../lib/likes';
import { CopyLinkButton } from '../ui/CopyLinkButton';
import { GlassHeroPanel } from '../ui/GlassHeroPanel';
import { AvatarArtifact } from './AvatarArtifact';
import { FollowBtn } from './FollowBtn';
import { StatOrb } from './StatOrb';
import { getWebIcon, InfoChip, ProChip, VerifiedBadge } from './UserChips';

function dateFormattedLong(dateStr: string | null | undefined) {
  if (!dateStr) return null;
  const d = new Date(dateStr.replace(/\//g, '-').replace(' +0000', 'Z'));
  if (Number.isNaN(d.getTime()) || d.getFullYear() <= 1970) return null;
  return d.toLocaleDateString(undefined, { year: 'numeric', month: 'long' });
}

interface IdentityHubProps {
  user: {
    urn: string;
    username: string;
    full_name?: string | null;
    description?: string | null;
    avatar_url?: string | null;
    permalink_url?: string | null;
    plan?: string | null;
    verified?: boolean;
    created_at?: string | null;
    city?: string | null;
    country_code?: string | null;
    followers_count?: number | null;
    followings_count?: number | null;
    track_count?: number | null;
    public_favorites_count?: number | null;
    likes_count?: number | null;
  };
  hasStar: boolean;
  webProfiles:
    | Array<{ id: number | string; url: string; service: string; title: string }>
    | undefined;
  aura: Aura;
  isOwnProfile: boolean;
  customHex: string;
  onPickAura: (a: Aura) => void;
  onPickCustom: (hex: string) => void;
}

/** Compact profile header — avatar + identity + stats in one tight block. */
export function IdentityHub({ user, hasStar, webProfiles, aura, isOwnProfile }: IdentityHubProps) {
  const { t } = useTranslation();
  const formattedDate = dateFormattedLong(user.created_at);
  const country = [user.city, user.country_code].filter(Boolean).join(', ');

  return (
    <GlassHeroPanel hasStar={hasStar} aura={aura}>
      <div className="relative flex flex-col items-center gap-4 p-4 sm:flex-row sm:items-start md:gap-6 md:p-6">
        <AvatarArtifact
          username={user.username}
          avatarUrl={user.avatar_url}
          hasStar={hasStar}
          aura={aura}
        />

        <div className="flex min-w-0 flex-1 flex-col gap-2.5 text-center sm:text-left">
          <div className="flex flex-wrap items-center justify-center gap-2 sm:justify-start">
            {user.verified && <VerifiedBadge title={t('user.verifiedArtist')} />}
            {user.plan && user.plan !== 'Free' && <ProChip plan={user.plan} />}
            {formattedDate && <InfoChip icon={<Calendar size={11} />}>{formattedDate}</InfoChip>}
            {country && <InfoChip icon={<Globe size={11} />}>{country}</InfoChip>}
          </div>

          <div className="flex flex-wrap items-baseline justify-center gap-x-3 gap-y-1 sm:justify-start">
            <h1 className="max-w-full break-words text-2xl font-black leading-tight tracking-tight text-white md:text-4xl">
              {user.username}
            </h1>
            {user.full_name && user.full_name !== user.username && (
              <p className="text-[12.5px] font-medium text-white/40">{user.full_name}</p>
            )}
          </div>

          {user.description && (
            <p className="selectable line-clamp-2 max-w-2xl text-[13px] leading-relaxed text-white/60">
              {user.description}
            </p>
          )}

          <div className="flex flex-wrap items-center justify-center gap-2 sm:justify-start">
            {!isOwnProfile && <FollowBtn userUrn={user.urn} aura={aura} />}
            {user.permalink_url && <CopyLinkButton url={user.permalink_url} size="sm" />}
            {webProfiles?.map((link) => (
              <a
                key={link.id}
                href={link.url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 rounded-full px-2.5 py-1.5 text-[11px] font-medium text-white/55 transition-colors hover:text-white"
                style={{
                  background: 'rgba(28,28,32,0.85)',
                  border: '0.5px solid rgba(255,255,255,0.08)',
                }}
              >
                <span className="text-white/45">{getWebIcon(link.service)}</span>
                <span className="max-w-[140px] truncate">{link.title}</span>
              </a>
            ))}
          </div>

          <div className="flex flex-wrap justify-center gap-2 pt-0.5 sm:justify-start">
            <StatOrb
              value={user.followers_count}
              label={t('user.followers')}
              accent={auraRgba(aura, 0.2)}
            />
            <StatOrb
              value={user.followings_count}
              label={t('user.following')}
              accent={auraRgba(aura, 0.16)}
            />
            <StatOrb
              value={user.track_count}
              label={t('user.tracks')}
              accent={auraRgba(aura, 0.14)}
            />
            <StatOrb
              value={likedTracksCount(user)}
              label={t('user.likes')}
              accent={auraRgba(aura, 0.12)}
            />
          </div>
        </div>
      </div>
    </GlassHeroPanel>
  );
}
