import React, { useMemo, useState } from 'react';
import { fc } from '../../lib/formatters';
import type { Comment } from '../../lib/hooks';
import { Loader2, MessageCircle } from '../../lib/icons';
import { useAuthStore } from '../../stores/auth';
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
  trackUrn,
  commentCount,
  comments,
  loading,
  fetchingMore,
  sentinelRef,
  aura,
  onSeek,
}: {
  trackUrn: string;
  commentCount?: number;
  comments: Comment[];
  loading: boolean;
  fetchingMore: boolean;
  sentinelRef: React.Ref<HTMLDivElement>;
  aura: TrackAura;
  onSeek: (seconds: number) => void;
}) {
  const [sort, setSort] = useState<CommentSort>('newest');
  const myUrn = useAuthStore((s) => s.user?.urn);

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
    <section className="space-y-1">
      <div className="flex items-center gap-3 pb-1">
        <h2 className="text-[16px] font-bold text-white/85">{'Comments'}</h2>
        {commentCount != null && (
          <span className="text-[12px] tabular-nums text-white/35">{fc(commentCount)}</span>
        )}
        <div className="ml-auto flex items-center gap-2 text-[11px] font-medium">
          {SORTS.map((opt, i) => {
            const on = sort === opt.id;
            return (
              <React.Fragment key={opt.id}>
                {i > 0 && <span className="text-white/15">{'·'}</span>}
                <button
                  type="button"
                  onClick={() => setSort(opt.id)}
                  className={`transition-colors cursor-pointer ${
                    on ? 'text-white/85' : 'text-white/35 hover:text-white/60'
                  }`}
                >
                  {opt.label}
                </button>
              </React.Fragment>
            );
          })}
        </div>
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
        <div>
          {sorted.map((c) => (
            <VoiceCard
              key={c.urn ?? c.id}
              comment={c}
              trackUrn={trackUrn}
              canDelete={!!myUrn && c.user?.urn === myUrn}
              accent={aura.accent}
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
