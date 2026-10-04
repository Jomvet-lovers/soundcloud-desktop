import React from 'react';
import { useTranslation } from 'react-i18next';
import { useNavigate } from 'react-router-dom';
import { art, dateFormatted, durLong } from '../../lib/formatters';
import type { Playlist } from '../../lib/hooks';
import { Calendar, Clock, Library, ListMusic } from '../../lib/icons';
import { Avatar } from '../ui/Avatar';
import { PlaylistActions } from './PlaylistActions';

function kindLabelKey(kind: string | undefined): { ns: string; defaultValue: string } {
  switch (kind) {
    case 'compilation':
      return { ns: 'playlist.kind.collection', defaultValue: 'Collection' };
    case 'album':
    case 'ep':
    case 'single':
      return { ns: `artist.kind.${kind}`, defaultValue: kind };
    default:
      return { ns: 'playlist.kind.set', defaultValue: 'Set' };
  }
}

/** Playlist header — mirrors the album hero layout (cover + title + author + meta). */
export const PlaylistHero = React.memo(function PlaylistHero({
  playlist,
  isOwner,
  isPlaying,
  isPinned,
  onPlayAll,
  onShuffle,
  onTogglePin,
  onDelete,
}: {
  playlist: Playlist;
  isOwner: boolean;
  isPlaying: boolean;
  isPinned: boolean;
  onPlayAll: () => void;
  onShuffle: () => void;
  onTogglePin: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const kl = kindLabelKey(playlist.kind);
  const cover = art(playlist.artwork_url, 't500x500');
  const curator = playlist.user;

  return (
    <div className="flex flex-col items-center gap-5 md:flex-row md:items-center md:gap-6">
      {/* Cover — same size and rounding as the album cover */}
      <div className="relative h-[180px] w-[180px] shrink-0 self-center md:h-[220px] md:w-[220px]">
        <div
          className="relative h-full w-full overflow-hidden rounded-xl"
          style={{
            background: 'rgba(255,255,255,0.03)',
            boxShadow:
              '0 25px 60px rgba(0,0,0,0.55), inset 0 0 0 1px rgba(255,255,255,0.08), inset 0 1px 0 rgba(255,255,255,0.1)',
          }}
        >
          {cover ? (
            <img src={cover} alt={playlist.title} className="h-full w-full object-cover" />
          ) : (
            <div className="flex h-full w-full items-center justify-center">
              <ListMusic size={64} className="text-white/15" />
            </div>
          )}
        </div>
      </div>

      <div className="flex w-full min-w-0 flex-1 flex-col gap-3 text-center md:text-left">
        <h1 className="max-w-full break-words text-3xl font-black leading-tight tracking-tight text-white md:text-5xl">
          {playlist.title}
        </h1>

        {curator && (
          <button
            type="button"
            onClick={() => navigate(`/user/${encodeURIComponent(curator.urn)}`)}
            className="group inline-flex items-center justify-center gap-2 cursor-pointer md:justify-start"
          >
            <span className="relative h-7 w-7 overflow-hidden rounded-full ring-1 ring-white/15">
              <Avatar src={curator.avatar_url} alt={curator.username} size={28} />
            </span>
            <span className="flex flex-col items-start leading-tight">
              <span className="text-[12px] font-semibold text-white/90 group-hover:text-white">
                {curator.username}
              </span>
              <span className="text-[10px] font-medium text-white/35">
                {t('playlist.curatedBy')}
              </span>
            </span>
          </button>
        )}

        <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-[11px] text-white/45 md:justify-start">
          <span className="inline-flex items-center gap-1.5">
            <Library size={11} /> {t(kl.ns, { defaultValue: kl.defaultValue })}
          </span>
          {playlist.duration > 0 && (
            <span className="inline-flex items-center gap-1.5">
              <Clock size={11} /> {durLong(playlist.duration)}
            </span>
          )}
          {playlist.last_modified && (
            <span className="inline-flex items-center gap-1.5">
              <Calendar size={11} />{' '}
              {t('playlist.lastEdited', { date: dateFormatted(playlist.last_modified) })}
            </span>
          )}
          {playlist.label_name && <span>{playlist.label_name}</span>}
        </div>

        <div className="flex flex-wrap items-center justify-center gap-3 pt-1 md:justify-start">
          <PlaylistActions
            playlist={playlist}
            isOwner={isOwner}
            isPlaying={isPlaying}
            isPinned={isPinned}
            onPlayAll={onPlayAll}
            onShuffle={onShuffle}
            onTogglePin={onTogglePin}
            onDelete={onDelete}
          />
        </div>

        {playlist.description && (
          <p className="selectable max-w-2xl text-[12.5px] leading-relaxed text-white/60">
            {playlist.description}
          </p>
        )}
      </div>
    </div>
  );
});
