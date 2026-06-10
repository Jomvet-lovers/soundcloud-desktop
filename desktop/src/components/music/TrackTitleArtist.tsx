import React from 'react';
import {useTranslation} from 'react-i18next';
import {useNavigate} from 'react-router-dom';
import {useArtistDisplay, useArtistLinkItems, useDisplayTitle} from '../../lib/track-display';
import type {Track} from '../../stores/player';
import {ArtistNameLinks} from './ArtistNameLinks';
import {UploadKindDot} from './UploadKindDot';

interface TrackTitleArtistProps {
  track: Track;
  highlight?: boolean;
  size?: 'sm' | 'md' | 'lg';
  className?: string;
}

const TITLE_CLS = {
  sm: 'text-[13px]',
  md: 'text-[14px]',
  lg: 'text-[16px]',
} as const;

const ARTIST_CLS = {
  sm: 'text-[11px]',
  md: 'text-[12px]',
  lg: 'text-[13px]',
} as const;

export const TrackTitleArtist = React.memo(function TrackTitleArtist({
  track,
  highlight,
  size = 'md',
  className,
}: TrackTitleArtistProps) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const artistDisplay = useArtistDisplay(track);
  const displayTitle = useDisplayTitle(track);
  const artistLinks = useArtistLinkItems(track);
  const isWanted = artistDisplay.availability !== 'indexed';

  return (
    <div className={`min-w-0 flex-1 ${className ?? ''}`}>
      <p
        className={`${TITLE_CLS[size]} font-medium truncate transition-colors duration-150 ${
          highlight
            ? 'text-accent drop-shadow-[0_0_8px_rgba(255,85,0,0.4)]'
            : isWanted
              ? 'text-white/55'
              : 'text-white/90 hover:text-white cursor-pointer'
        }`}
        onClick={isWanted ? undefined : () => navigate(`/track/${encodeURIComponent(track.urn)}`)}
      >
        {displayTitle}
      </p>
      <p
        className={`${ARTIST_CLS[size]} truncate mt-0.5 flex items-center gap-1 ${
          isWanted ? 'text-white/30' : 'text-white/40'
        } transition-colors`}
      >
        <UploadKindDot kind={artistDisplay.uploadKind} />
        <span className="truncate">
          {isWanted ? (
            artistDisplay.primary
          ) : (
            <ArtistNameLinks
              items={artistLinks}
              linkClassName="cursor-pointer transition-colors hover:text-white/75"
            />
          )}
        </span>
        {isWanted && (
          <span className="text-[10px] text-white/25 ml-1">
            · {t('track.notFoundOnSc', 'not found on SoundCloud')}
          </span>
        )}
      </p>
    </div>
  );
});
