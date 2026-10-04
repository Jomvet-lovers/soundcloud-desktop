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
    <div className="relative shrink-0 self-center lg:self-start w-[148px] h-[148px] md:w-[180px] md:h-[180px]">
      <div
        className="relative w-full h-full rounded-[2rem] overflow-hidden"
        style={{
          background: 'rgba(255,255,255,0.03)',
          border: '0.5px solid rgba(255,255,255,0.10)',
        }}
      >
        {url ? (
          <img src={url} alt={username} className="w-full h-full object-cover" />
        ) : (
          <div className="w-full h-full flex items-center justify-center">
            <Users size={56} className="text-white/15" />
          </div>
        )}
      </div>
    </div>
  );
}

export const AvatarArtifact = React.memo(AvatarArtifactImpl);
