import type { Aura } from '../../lib/aura';
import { fc } from '../../lib/formatters';
import { Calendar, Globe } from '../../lib/icons';
import { likedTracksCount } from '../../lib/likes';
import { CopyLinkButton } from '../ui/CopyLinkButton';
import { AvatarArtifact } from './AvatarArtifact';
import { FollowBtn } from './FollowBtn';
import { getWebIcon, ProChip, VerifiedBadge } from './UserChips';

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

/** Plain inline stat — no boxes, just number + label. */
function Stat({ value, label }: { value?: number | null; label: string }) {
  return (
    <span className="whitespace-nowrap">
      <span className="font-semibold text-white/90">{value != null ? fc(value) : '—'}</span>{' '}
      <span className="text-white/40">{label}</span>
    </span>
  );
}

/** Compact profile header — one tight block, no framed stat boxes. */
export function IdentityHub({ user, hasStar, webProfiles, aura, isOwnProfile }: IdentityHubProps) {
  const formattedDate = dateFormattedLong(user.created_at);
  const country = [user.city, user.country_code].filter(Boolean).join(', ');

  return (
    <div className="flex flex-col">
      <div className="relative flex flex-col items-center gap-4 sm:flex-row sm:items-start md:gap-5">
        <AvatarArtifact
          username={user.username}
          avatarUrl={user.avatar_url}
          hasStar={hasStar}
          aura={aura}
        />

        <div className="flex min-w-0 flex-1 flex-col gap-2 text-center sm:text-left">
          <div className="flex flex-wrap items-center justify-center gap-x-2.5 gap-y-1 sm:justify-start">
            <h1 className="max-w-full break-words text-xl font-black leading-tight tracking-tight text-white md:text-3xl">
              {user.username}
            </h1>
            {user.verified && <VerifiedBadge title={'Verified Artist'} />}
            {user.plan && user.plan !== 'Free' && <ProChip plan={user.plan} />}
            {formattedDate && (
              <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-white/55">
                <Calendar size={11} className="text-white/45" /> {formattedDate}
              </span>
            )}
            {country && (
              <span className="inline-flex items-center gap-1.5 text-[11px] font-medium text-white/55">
                <Globe size={11} className="text-white/45" /> {country}
              </span>
            )}
            {user.full_name && user.full_name !== user.username && (
              <p className="text-[12px] font-medium text-white/40">{user.full_name}</p>
            )}
          </div>

          {user.description && (
            <p className="selectable line-clamp-2 max-w-2xl text-[12.5px] leading-relaxed text-white/60">
              {user.description}
            </p>
          )}

          <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-[12.5px] sm:justify-start">
            <Stat value={user.followers_count} label={'Followers'} />
            <Stat value={user.followings_count} label={'Following'} />
            <Stat value={user.track_count} label={'Tracks'} />
            <Stat value={likedTracksCount(user)} label={'Likes'} />
          </div>

          <div className="flex flex-wrap items-center justify-center gap-2 pt-0.5 sm:justify-start">
            {!isOwnProfile && <FollowBtn userUrn={user.urn} aura={aura} />}
            {user.permalink_url && <CopyLinkButton url={user.permalink_url} size="sm" />}
            {webProfiles?.map((link) => (
              <a
                key={link.id ?? link.url}
                href={link.url}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 text-[11px] font-medium text-white/55 transition-colors hover:text-white"
              >
                <span className="text-white/45">{getWebIcon(link.service)}</span>
                <span className="max-w-[140px] truncate">{link.title}</span>
              </a>
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}
