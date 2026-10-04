import React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { type Aura, auraRgba } from '../../lib/aura';
import { fc } from '../../lib/formatters';
import type { Playlist } from '../../lib/hooks';
import { useUser } from '../../lib/hooks';
import { Avatar } from '../ui/Avatar';
import { FollowBtn } from '../user/FollowBtn';

type Curator = Playlist['user'];

/** The curator is the author — taste over a release credit. The playlist payload
 *  carries no avatar/stats, so we hydrate them from the curator's profile. */
export const CuratorCard = React.memo(function CuratorCard({
  user,
  aura,
  isOwner,
  note,
}: {
  user: Curator;
  aura: Aura;
  isOwner: boolean;
  note?: string | null;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { data: profile } = useUser(user.urn);
  const goUser = () => navigate(`/user/${encodeURIComponent(user.urn)}`);

  const avatarUrl = profile?.avatar_url ?? user.avatar_url;
  const followers = profile?.followers_count ?? user.followers_count;
  const trackCount = profile?.track_count ?? user.track_count;
  const trimmed = note?.trim();

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center gap-3.5">
        <button type="button" onClick={goUser} className="shrink-0 cursor-pointer">
          <Avatar
            src={avatarUrl}
            alt={user.username}
            size={48}
            className="ring-1 ring-white/[0.1]"
          />
        </button>
        <div className="min-w-0 flex-1">
          <p className="text-[10px] font-medium text-white/35">{t('playlist.curatedBy')}</p>
          <button
            type="button"
            onClick={goUser}
            className="block text-[16px] font-bold text-white/90 truncate hover:text-white transition-colors cursor-pointer text-left max-w-full"
          >
            {user.username}
          </button>
        </div>
        {!isOwner && <FollowBtn userUrn={user.urn} aura={aura} />}
      </div>

      {(followers != null || trackCount != null) && (
        <div className="flex flex-wrap gap-x-5 gap-y-2 text-[12.5px]">
          {followers != null && (
            <span className="whitespace-nowrap">
              <span className="font-semibold text-white/90">{fc(followers)}</span>{' '}
              <span className="text-white/40">{t('user.followers')}</span>
            </span>
          )}
          {trackCount != null && (
            <span className="whitespace-nowrap">
              <span className="font-semibold text-white/90">{fc(trackCount)}</span>{' '}
              <span className="text-white/40">{t('user.tracks')}</span>
            </span>
          )}
        </div>
      )}

      {trimmed && (
        <div className="pl-3.5" style={{ borderLeft: `2px solid ${auraRgba(aura, 0.45)}` }}>
          <p className="text-[9px] font-medium text-white/30 mb-1.5">{t('playlist.linerNote')}</p>
          <p className="selectable text-[12.5px] text-white/55 leading-relaxed whitespace-pre-wrap break-words line-clamp-4 hover:line-clamp-none transition-all duration-500">
            {trimmed}
          </p>
        </div>
      )}
    </div>
  );
});
