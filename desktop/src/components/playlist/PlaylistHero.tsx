import React from 'react';
import { useNavigate } from 'react-router-dom';
import { art, dateFormatted, durLong } from '../../lib/formatters';
import type { Playlist } from '../../lib/hooks';
import { Calendar, Clock, Library, ListMusic, Pause, Play } from '../../lib/icons';
import { Avatar } from '../ui/Avatar';
import { PlaylistActions } from './PlaylistActions';

function kindLabel(kind: string | undefined): string {
  switch (kind) {
    case 'compilation':
      return 'Collection';
    case 'album':
      return 'Album';
    case 'ep':
      return 'EP';
    case 'single':
      return 'Single';
    default:
      return 'Set';
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
  const navigate = useNavigate();
  const kl = kindLabel(playlist.kind);
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
        <div className="flex items-center gap-5 justify-center md:justify-start">
          {/* Play — the same circular control as the track page. */}
          <button
            type="button"
            onClick={onPlayAll}
            aria-label={isPlaying ? 'Pause' : 'Play'}
            className="w-[68px] h-[68px] shrink-0 rounded-full border border-white/[0.18] hover:border-white/[0.4] flex items-center justify-center text-white/90 hover:text-white transition-colors cursor-pointer"
          >
            {isPlaying ? (
              <Pause size={24} fill="currentColor" strokeWidth={0} />
            ) : (
              <Play size={24} fill="currentColor" strokeWidth={0} className="ml-1" />
            )}
          </button>
          <h1 className="max-w-full break-words text-3xl font-black leading-tight tracking-tight text-white md:text-5xl">
            {playlist.title}
          </h1>
        </div>

        {curator && (
          <button
            type="button"
            onClick={() => navigate(`/user/${encodeURIComponent(curator.urn)}`)}
            className="group inline-flex items-center justify-center gap-2 cursor-pointer md:justify-start"
          >
            <span className="relative h-7 w-7 overflow-hidden rounded-full ring-1 ring-white/15">
              <Avatar src={curator.avatar_url} alt={curator.username} size={28} />
            </span>
            <span className="text-[12px] leading-tight text-white/50">
              {'Curated by '}
              <span className="font-semibold text-white/90 group-hover:text-white">
                {curator.username}
              </span>
            </span>
          </button>
        )}

        <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-[11px] text-white/45 md:justify-start">
          <span className="inline-flex items-center gap-1.5">
            <Library size={11} /> {kl}
          </span>
          {playlist.duration > 0 && (
            <span className="inline-flex items-center gap-1.5">
              <Clock size={11} /> {durLong(playlist.duration)}
            </span>
          )}
          {playlist.last_modified && (
            <span className="inline-flex items-center gap-1.5">
              <Calendar size={11} /> {`Updated ${dateFormatted(playlist.last_modified)}`}
            </span>
          )}
          {playlist.label_name && <span>{playlist.label_name}</span>}
        </div>

        <div className="flex flex-wrap items-center justify-center gap-3 pt-1 md:justify-start">
          <PlaylistActions
            playlist={playlist}
            isOwner={isOwner}
            isPinned={isPinned}
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
