import React from 'react';
import type { Aura } from '../../lib/aura';
import { art } from '../../lib/formatters';
import { Users } from '../../lib/icons';

interface AvatarArtifactProps {
  username: string;
  avatarUrl: string | null | undefined;
  hasStar: boolean;
  aura: Aura;
}

function AvatarArtifactImpl(props: AvatarArtifactProps) {
  const { username, avatarUrl } = props;
  const url = art(avatarUrl, 't500x500');
  return (
    <div className="relative shrink-0 self-center w-[80px] h-[80px] md:w-[96px] md:h-[96px]">
      <div
        className="relative w-full h-full rounded-[1.25rem] overflow-hidden"
        style={{
          background: 'color-mix(in srgb, var(--ov-ink) 3%, transparent)',
          border: '0.5px solid color-mix(in srgb, var(--ov-ink) 10%, transparent)',
        }}
      >
        {url ? (
          <img src={url} alt={username} className="w-full h-full object-cover" />
        ) : (
          <div className="w-full h-full flex items-center justify-center">
            <Users size={32} className="text-white/15" />
          </div>
        )}
      </div>
    </div>
  );
}

export const AvatarArtifact = React.memo(AvatarArtifactImpl);
