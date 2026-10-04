import React from 'react';
import { useTranslation } from 'react-i18next';
import { dateFormatted, durLong } from '../../lib/formatters';
import type { Playlist } from '../../lib/hooks';
import { Calendar, Clock, Library } from '../../lib/icons';
import type { Track } from '../../stores/player';
import { CrateStack } from './CrateStack';
import { CuratorCard } from './CuratorCard';
import { PlaylistActions } from './PlaylistActions';
import type { PlaylistAura } from './usePlaylistAura';

function Meta({ icon, children }: { icon?: React.ReactNode; children: React.ReactNode }) {
  return (
    <span className="inline-flex items-center gap-1.5 text-[10px] font-semibold text-white/55">
      {icon && <span className="text-white/45">{icon}</span>}
      {children}
    </span>
  );
}

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

export const PlaylistHero = React.memo(function PlaylistHero({
  playlist,
  tracks,
  aura,
  isOwner,
  isPlaying,
  isPinned,
  trackCount,
  onPlayAll,
  onShuffle,
  onTogglePin,
  onDelete,
}: {
  playlist: Playlist;
  tracks: Track[];
  aura: PlaylistAura;
  isOwner: boolean;
  isPlaying: boolean;
  isPinned: boolean;
  trackCount: number;
  onPlayAll: () => void;
  onShuffle: () => void;
  onTogglePin: () => void;
  onDelete: () => void;
}) {
  const { t } = useTranslation();
  const kl = kindLabelKey(playlist.kind);
  const hasGenres = aura.topGenres.length > 0;

  return (
    <div className="flex flex-col items-center gap-6 lg:flex-row lg:items-center lg:gap-10">
      <CrateStack
        playlist={playlist}
        tracks={tracks}
        isPlaying={isPlaying}
        trackCount={trackCount}
        onPlay={onPlayAll}
      />

      <div className="flex w-full min-w-0 flex-1 flex-col gap-3 text-center lg:text-left">
        <h1 className="max-w-full break-words text-3xl font-black leading-tight tracking-tight text-white md:text-5xl">
          {playlist.title}
        </h1>

        {hasGenres && (
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1.5 justify-center lg:justify-start">
            {aura.topGenres.map((g) => (
              <span
                key={g.genre}
                className="inline-flex items-center gap-1.5 text-[11px] text-white/45"
              >
                <span className="w-2 h-2 rounded-full" style={{ background: g.color }} />
                {g.genre}
              </span>
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-center gap-x-4 gap-y-1 justify-center text-[11px] text-white/45 lg:justify-start">
          <Meta icon={<Library size={11} />}>{t(kl.ns, { defaultValue: kl.defaultValue })}</Meta>
          {aura.topGenres.length > 1 && (
            <Meta>{t('playlist.spansGenres', { count: aura.topGenres.length })}</Meta>
          )}
          {playlist.duration > 0 && (
            <Meta icon={<Clock size={11} />}>{durLong(playlist.duration)}</Meta>
          )}
          {playlist.last_modified && (
            <Meta icon={<Calendar size={11} />}>
              {t('playlist.lastEdited', { date: dateFormatted(playlist.last_modified) })}
            </Meta>
          )}
          {playlist.label_name && <Meta>{playlist.label_name}</Meta>}
        </div>

        <div className="pt-1">
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

        <CuratorCard
          user={playlist.user}
          aura={aura.aura}
          isOwner={isOwner}
          note={playlist.description}
        />
      </div>
    </div>
  );
});
