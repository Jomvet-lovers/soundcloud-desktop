import React from 'react';
import { useNavigate } from 'react-router-dom';
import { art, fc } from '../../lib/formatters';
import type { SCUser } from '../../lib/hooks';
import { User, Users } from '../../lib/icons';

export const UserCard = React.memo(({ user }: { user: SCUser }) => {
  const navigate = useNavigate();
  const avatar = art(user.avatar_url, 't300x300');

  return (
    <div
      className="group flex flex-col items-center gap-2 p-3 cursor-pointer"
      onClick={() => navigate(`/user/${encodeURIComponent(user.urn)}`)}
    >
      <div className="relative h-16 w-16 overflow-hidden rounded-full ring-1 ring-white/[0.05] transition-colors group-hover:ring-white/[0.15]">
        {avatar ? (
          <img
            src={avatar}
            alt={user.username}
            className="w-full h-full object-cover"
            decoding="async"
          />
        ) : (
          <div className="w-full h-full bg-white/5 flex items-center justify-center">
            <User size={24} className="text-white/20" />
          </div>
        )}
      </div>

      <div className="text-center w-full">
        <p className="truncate text-[13px] font-medium text-white/85 group-hover:text-white transition-colors">
          {user.username}
        </p>
        <div className="flex items-center justify-center gap-3 mt-0.5 text-[10.5px] text-white/30">
          <span className="flex items-center gap-1">
            <Users size={10} />
            {fc(user.followers_count)}
          </span>
        </div>
      </div>
    </div>
  );
});
