import React, { useMemo, useState } from 'react';
import { fc } from '../../lib/formatters';
import type { Comment } from '../../lib/hooks';
import { Loader2, MessageCircle } from '../../lib/icons';
import { VoiceCard } from './comments';
import type { TrackAura } from './useTrackAura';

type CommentSort = 'newest' | 'timeline';

const SORTS: Array<{ id: CommentSort; label: string }> = [
  { id: 'newest', label: 'Newest' },
  { id: 'timeline', label: 'Timeline' },
];

/** Listeners' voices — the sticky rail's comment wall. Every timestamped
 *  comment is a clickable jump-cut into the song; synced with SoundCloud. */
export const RoomVoices = React.memo(function RoomVoices({
  commentCount,
  comments,
  loading,
  fetchingMore,
  sentinelRef,
  aura,
  onSeek,
}: {
  commentCount?: number;
  comments: Comment[];
  loading: boolean;
  fetchingMore: boolean;
  sentinelRef: React.Ref<HTMLDivElement>;
  aura: TrackAura;
  onSeek: (seconds: number) => void;
}) {
  const [sort, setSort] = useState<CommentSort>('newest');

  const sorted = useMemo(() => {
    const list = [...comments];
    if (sort === 'timeline') {
      list.sort(
        (a, b) =>
          (a.timestamp ?? Number.MAX_SAFE_INTEGER) - (b.timestamp ?? Number.MAX_SAFE_INTEGER),
      );
    } else {
      list.sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at));
    }
    return list;
  }, [comments, sort]);

  return (
    <section className="space-y-4">
      <div className="flex items-center gap-3">
        <span
          className="w-7 h-7 rounded-full flex items-center justify-center shrink-0"
          style={{ background: aura.accentSoft, color: aura.accent }}
        >
          <MessageCircle size={14} />
        </span>
        <h2 className="text-[16px] font-bold text-white/85">{'Comments'}</h2>
        {commentCount != null && (
          <span
            className="text-[11px] font-semibold tabular-nums px-2.5 h-6 inline-flex items-center rounded-full text-white/45"
            style={{ background: 'rgba(255,255,255,0.05)' }}
          >
            {fc(commentCount)}
          </span>
        )}
      </div>

      <div className="flex items-center gap-1">
        {SORTS.map((opt) => {
          const on = sort === opt.id;
          return (
            <button
              key={opt.id}
              type="button"
              onClick={() => setSort(opt.id)}
              className={`relative rounded-lg px-2.5 py-1.5 text-[11px] font-medium transition-colors cursor-pointer ${
                on ? 'text-white/90' : 'text-white/40 hover:text-white/70'
              }`}
            >
              <span
                aria-hidden
                className={`pointer-events-none absolute inset-0 rounded-lg bg-white/[0.1] transition-opacity duration-200 ${
                  on ? 'opacity-100' : 'opacity-0'
                }`}
              />
              <span className="relative">{opt.label}</span>
            </button>
          );
        })}
      </div>

      {loading ? (
        <div className="flex justify-center py-10">
          <Loader2 size={18} className="text-white/15 animate-spin" />
        </div>
      ) : sorted.length === 0 ? (
        <div className="py-14 flex flex-col items-center gap-4">
          <MessageCircle size={24} className="text-white/15" />
          <p className="text-white/30 text-sm">{'No comments yet'}</p>
        </div>
      ) : (
        <div className="space-y-3">
          {sorted.map((c) => (
            <VoiceCard
              key={c.urn ?? c.id}
              comment={c}
              accent={aura.accent}
              accentSoft={aura.accentSoft}
              accentGlow={aura.accentGlow}
              onSeek={onSeek}
            />
          ))}
          <div ref={sentinelRef} className="h-4 flex items-center justify-center">
            {fetchingMore && <Loader2 size={14} className="text-white/15 animate-spin" />}
          </div>
        </div>
      )}
    </section>
  );
});
