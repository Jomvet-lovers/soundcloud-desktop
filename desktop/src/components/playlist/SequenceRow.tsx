import { useSortable } from '@dnd-kit/sortable';
import { CSS } from '@dnd-kit/utilities';
import React from 'react';
import { preloadTrack } from '../../lib/audio';
import { art, dur, fc } from '../../lib/formatters';
import {
  GripVertical,
  headphones9,
  heart9,
  musicIcon12,
  playWhite12,
  Trash2,
} from '../../lib/icons';
import { useTrackPlay } from '../../lib/useTrackPlay';
import type { Track } from '../../stores/player';
import { trackMenuHandler } from '../../stores/track-menu';
import { LikeButton } from '../music/LikeButton';
import { PlayingBars } from '../music/PlayingBars';
import { sameScdMeta, TrackStatusBadges } from '../music/TrackStatusBadges';
import { TrackTitleArtist } from '../music/TrackTitleArtist';

/** Shared row body. */
function RowBody({
  track,
  index,
  isThis,
  isThisPlaying,
  togglePlay,
}: {
  track: Track;
  index: number;
  isThis: boolean;
  isThisPlaying: boolean;
  togglePlay: () => void;
}) {
  const cover = art(track.artwork_url, 't200x200');

  return (
    <>
      <div
        className="w-9 h-9 flex items-center justify-center shrink-0 cursor-pointer"
        onClick={togglePlay}
        onMouseEnter={() => preloadTrack(track.urn)}
      >
        {isThisPlaying ? (
          <PlayingBars playing={isThisPlaying} />
        ) : (
          <>
            <span className="text-[12px] text-white/25 tabular-nums font-medium group-hover:hidden">
              {index + 1}
            </span>
            <div className="hidden group-hover:flex w-8 h-8 rounded-full bg-white/10 items-center justify-center">
              {playWhite12}
            </div>
          </>
        )}
      </div>

      <div className="relative w-9 h-9 rounded-lg overflow-hidden shrink-0 ring-1 ring-white/[0.06]">
        {cover ? (
          <img
            src={cover}
            alt=""
            className="w-full h-full object-cover"
            decoding="async"
            loading="lazy"
          />
        ) : (
          <div className="w-full h-full flex items-center justify-center bg-white/[0.03]">
            {musicIcon12}
          </div>
        )}
      </div>

      <TrackTitleArtist track={track} highlight={isThis} size="sm" />

      <div className="hidden sm:flex shrink-0">
        <TrackStatusBadges meta={track._scd_meta} />
      </div>

      <div className="hidden sm:flex items-center gap-3 shrink-0">
        {track.playback_count != null && (
          <span className="text-[10px] text-white/20 tabular-nums flex items-center gap-0.5">
            {headphones9}
            {fc(track.playback_count)}
          </span>
        )}
        {(track.favoritings_count ?? track.likes_count) != null && (
          <span className="text-[10px] text-white/20 tabular-nums flex items-center gap-0.5">
            {heart9}
            {fc(track.favoritings_count ?? track.likes_count)}
          </span>
        )}
      </div>

      <LikeButton track={track} />

      <span className="text-[11px] text-white/25 tabular-nums font-medium shrink-0 w-10 text-right">
        {dur(track.duration)}
      </span>
    </>
  );
}

const ROW_BASE =
  'group relative flex items-center gap-3.5 pl-4 pr-4 py-3 rounded-md transition-colors duration-150 ease-[var(--ease-apple)] select-none';

function activeCls(_isThis: boolean) {
  return 'hover:bg-white/[0.03]';
}

/** Owner row — drag-to-reorder with a "pulled sleeve" tilt; remove on hover. */
export const SortableSequenceRow = React.memo(
  function SortableSequenceRow({
    track,
    index,
    queue,
    onRemove,
    onPlay,
  }: {
    track: Track;
    index: number;
    queue: Track[];
    onRemove: (urn: string) => void;
    onPlay?: () => void;
  }) {
    const { isThis, isThisPlaying, togglePlay } = useTrackPlay(track, queue, onPlay);
    const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
      id: track.urn,
    });
    const base = CSS.Transform.toString(transform);

    return (
      <div
        ref={setNodeRef}
        style={{
          transform: base,
          transition,
          // Source row is hidden while dragging — the DragOverlay renders the
          // floating copy, so the drag survives this row windowing out of the
          // virtual list. (No content-visibility: the VirtualList already windows;
          // stacking it caused the scroll jank.)
          opacity: isDragging ? 0 : 1,
        }}
        className={`${ROW_BASE} ${activeCls(isThis)}`}
        onContextMenu={trackMenuHandler(track)}
      >
        <div
          className="w-5 flex items-center justify-center shrink-0 cursor-grab active:cursor-grabbing text-white/15 hover:text-white/40 transition-colors -ml-1"
          {...attributes}
          {...listeners}
        >
          <GripVertical size={14} />
        </div>

        <RowBody
          track={track}
          index={index}
          isThis={isThis}
          isThisPlaying={isThisPlaying}
          togglePlay={togglePlay}
        />

        <button
          type="button"
          onClick={() => onRemove(track.urn)}
          className="opacity-0 group-hover:opacity-100 w-7 h-7 rounded-lg flex items-center justify-center text-white/20 hover:text-red-400 hover:bg-red-400/10 transition-all duration-200 shrink-0"
          title={'Remove from playlist'}
        >
          <Trash2 size={13} />
        </button>
      </div>
    );
  },
  (prev, next) =>
    prev.track.urn === next.track.urn &&
    prev.index === next.index &&
    prev.track.user_favorite === next.track.user_favorite &&
    prev.track.title === next.track.title &&
    prev.track.enrichment === next.track.enrichment &&
    sameScdMeta(prev.track._scd_meta, next.track._scd_meta),
);

/** Floating copy rendered by DragOverlay while reordering — stays mounted even if
 *  the source row windows out of the virtual list, so long auto-scroll drags work. */
export function SequenceRowOverlay({
  track,
  index,
  queue,
}: {
  track: Track;
  index: number;
  queue: Track[];
}) {
  const { isThis, isThisPlaying, togglePlay } = useTrackPlay(track, queue);
  return (
    <div
      className={`${ROW_BASE} ${activeCls(isThis)} bg-white/[0.06] cursor-grabbing`}
      style={{
        transform: 'rotate(1.5deg)',
        boxShadow: '0 24px 60px rgba(0,0,0,0.55), inset 0 0 0 0.5px rgba(255,255,255,0.12)',
      }}
    >
      <div className="w-5 flex items-center justify-center shrink-0 text-white/40 -ml-1">
        <GripVertical size={14} />
      </div>
      <RowBody
        track={track}
        index={index}
        isThis={isThis}
        isThisPlaying={isThisPlaying}
        togglePlay={togglePlay}
      />
    </div>
  );
}

/** Read-only row (non-owner). */
export const SequenceRow = React.memo(
  function SequenceRow({
    track,
    index,
    queue,
    onPlay,
  }: {
    track: Track;
    index: number;
    queue: Track[];
    onPlay?: () => void;
  }) {
    const { isThis, isThisPlaying, togglePlay } = useTrackPlay(track, queue, onPlay);
    return (
      <div className={`${ROW_BASE} ${activeCls(isThis)}`} onContextMenu={trackMenuHandler(track)}>
        <RowBody
          track={track}
          index={index}
          isThis={isThis}
          isThisPlaying={isThisPlaying}
          togglePlay={togglePlay}
        />
      </div>
    );
  },
  (prev, next) =>
    prev.track.urn === next.track.urn &&
    prev.index === next.index &&
    prev.track.user_favorite === next.track.user_favorite &&
    prev.track.title === next.track.title &&
    prev.track.enrichment === next.track.enrichment &&
    sameScdMeta(prev.track._scd_meta, next.track._scd_meta),
);
