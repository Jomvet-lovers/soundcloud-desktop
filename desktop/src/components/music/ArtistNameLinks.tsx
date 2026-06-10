import React from 'react';
import {useNavigate} from 'react-router-dom';
import type {ArtistLinkItem} from '../../lib/track-display';

/**
 * Строка авторов «A, B, C», где каждое имя — отдельная ссылка на своего
 * артиста/юзера (item.target из `getArtistLinkItems`). Несматченные имена —
 * обычный текст.
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
      {items.map((it, i) => (
        <React.Fragment key={`${it.name}-${i}`}>
          {i > 0 && ', '}
          <span
            className={
              it.target
                ? (linkClassName ?? 'cursor-pointer transition-colors hover:text-white/85')
                : undefined
            }
            onClick={
              it.target
                ? (e) => {
                    e.stopPropagation();
                    navigate(it.target as string);
                  }
                : undefined
            }
          >
            {it.name}
          </span>
        </React.Fragment>
      ))}
    </>
  );
});
