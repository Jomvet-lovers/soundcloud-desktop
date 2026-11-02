import React from 'react';
import { useNavigate } from 'react-router-dom';
import type { ArtistLinkItem } from '../../lib/track-display';
import { userMenuHandler } from '../../stores/track-menu';

/**
 * Artist line "A, B, C" where each name is a separate link to ITS OWN
 * artist/user (item.target from `getArtistLinkItems`). Unmatched names stay
 * plain text. stopPropagation — rows are clickable themselves (play/context).
 */
export const ArtistNameLinks = React.memo(function ArtistNameLinks({
  items,
  linkClassName,
}: {
  items: ArtistLinkItem[];
  linkClassName?: string;
}) {
  const navigate = useNavigate();
  return (
    <>
      {items.map((it, i) => {
        const target = it.target;
        const go = target
          ? (e: React.SyntheticEvent) => {
              e.stopPropagation();
              navigate(target);
            }
          : undefined;
        return (
          <React.Fragment key={it.name}>
            {i > 0 && ', '}
            {go ? (
              <span
                role="link"
                tabIndex={0}
                className={linkClassName ?? 'cursor-pointer transition-colors hover:text-white/85'}
                onClick={go}
                onContextMenu={target ? userMenuHandler({ target }) : undefined}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') go(e);
                }}
              >
                {it.name}
              </span>
            ) : (
              <span>{it.name}</span>
            )}
          </React.Fragment>
        );
      })}
    </>
  );
});
