import React, { useEffect, useMemo, useRef, useState } from 'react';
import {
  clearCommentsTimeline,
  getCurrentTime,
  getDuration,
  setCommentsTimeline,
  subscribe,
  subscribeFloatingComments,
  type TimelineComment,
} from '../../lib/audio';
import { art } from '../../lib/formatters';
import type { Comment } from '../../lib/hooks';
import { useSettingsStore } from '../../stores/settings';

interface Dot {
  pct: number;
  comment: Comment;
  count: number;
}

const SLOTS = 46;
const BLOOM_MS = 4600;

function CommentAvatar({
  url,
  username,
  className,
}: {
  url: string | null;
  username: string | null;
  className: string;
}) {
  const avatar = art(url, 'small');
  if (avatar) {
    return <img src={avatar} alt="" loading="lazy" className={`${className} object-cover`} />;
  }
  return (
    <span className={`${className} flex items-center justify-center bg-white/10 text-white/65`}>
      {(username || '?').slice(0, 1).toUpperCase()}
    </span>
  );
}

/** Comments live ON the wave: a small avatar at each timestamped comment's
 *  moment. Hover peeks it; click jumps there; and while this track plays, a
 *  bubble blooms downward as the playhead sweeps past it — all DOM-driven, no
 *  per-frame React. */
export const WaveVoices = React.memo(function WaveVoices({
  comments,
  durationMs,
  isCurrent,
  onSeek,
}: {
  comments: Comment[];
  durationMs: number;
  isCurrent: boolean;
  onSeek: (seconds: number) => void;
}) {
  // When floating pills are on they already announce each comment at its
  // moment — blooming the wave pill too would show it twice.
  const floatingOn = useSettingsStore((s) => s.floatingComments);
  const dots = useMemo<Dot[]>(() => {
    if (!durationMs || durationMs <= 0) return [];
    const slots = new Map<number, Dot>();
    for (const c of comments) {
      if (c.timestamp == null || !c.body) continue;
      const pct = Math.min(1, Math.max(0, c.timestamp / durationMs));
      const slot = Math.min(SLOTS - 1, Math.round(pct * SLOTS));
      const hit = slots.get(slot);
      if (hit) hit.count++;
      else slots.set(slot, { pct, comment: c, count: 1 });
    }
    return [...slots.values()].sort((a, b) => a.pct - b.pct);
  }, [comments, durationMs]);

  const elsRef = useRef<(HTMLElement | null)[]>([]);
  const timersRef = useRef<Map<number, ReturnType<typeof setTimeout>>>(new Map());

  // Bloom a dot when the live playhead sweeps past it (natural advance only —
  // a seek jump is skipped so we don't burst-bloom). Skipped while floating
  // pills are on (they cover the same moment). Pure DOM, no React.
  useEffect(() => {
    const timers = timersRef.current;
    if (!isCurrent || floatingOn || dots.length === 0) return;
    let prev = (() => {
      const d = getDuration();
      return d > 0 ? getCurrentTime() / d : 0;
    })();

    const tick = () => {
      const d = getDuration();
      if (d <= 0) return;
      const cur = Math.min(1, Math.max(0, getCurrentTime() / d));
      const delta = cur - prev;
      if (delta > 0 && delta < 0.03) {
        for (let i = 0; i < dots.length; i++) {
          if (dots[i].pct > prev && dots[i].pct <= cur) bloom(i);
        }
      }
      prev = cur;
    };

    const bloom = (i: number) => {
      const el = elsRef.current[i];
      if (!el) return;
      el.dataset.bloom = '1';
      const old = timers.get(i);
      if (old) clearTimeout(old);
      timers.set(
        i,
        setTimeout(() => {
          if (elsRef.current[i]) elsRef.current[i]!.dataset.bloom = '0';
          timers.delete(i);
        }, BLOOM_MS),
      );
    };

    const unsub = subscribe(tick);
    return () => {
      unsub();
      for (const id of timers.values()) clearTimeout(id);
      timers.clear();
    };
  }, [isCurrent, floatingOn, dots]);

  if (dots.length === 0) return null;

  return (
    <div className="absolute inset-0 pointer-events-none z-10">
      {dots.map((d, i) => {
        const user = d.comment.user;
        return (
          <button
            key={d.comment.urn ?? d.comment.id}
            type="button"
            data-bloom="0"
            ref={(el) => {
              elsRef.current[i] = el;
            }}
            onClick={() => onSeek((d.comment.timestamp ?? 0) / 1000)}
            className="wv-dot group/dot absolute top-[75%] -translate-y-1/2 pointer-events-auto cursor-pointer"
            style={{ left: `${d.pct * 100}%` }}
          >
            <span className="wv-pip-wrap relative block w-6 h-6 -translate-x-1/2 transition-transform duration-200 group-hover/dot:scale-125">
              <span className="wv-pip block size-full overflow-hidden rounded-full bg-white/10 transition-transform duration-200">
                <CommentAvatar
                  url={user?.avatar_url ?? null}
                  username={user?.username ?? null}
                  className="size-full text-[10px] font-semibold"
                />
              </span>
              {d.count > 1 && (
                <span className="absolute -top-1 -right-1 min-w-[12px] h-[12px] px-0.5 rounded-full bg-black/85 text-[8px] font-semibold text-white/85 flex items-center justify-center leading-none">
                  {d.count}
                </span>
              )}
            </span>
            {/* Peek / playback bloom — the comment itself (no avatar, no
                byline), hanging below the dot. Same look as .scd-tooltip. */}
            <span
              className="wv-pill absolute left-0 z-20 block w-max max-w-[320px] text-left font-medium leading-snug break-words"
              style={{
                background: '#1b1b1f',
                border: '0.5px solid rgba(255,255,255,0.1)',
                borderRadius: 7,
                boxShadow: '0 6px 18px rgba(0,0,0,0.45)',
                color: 'rgba(255,255,255,0.88)',
                fontSize: 11.5,
                padding: '6px 9px',
                overflowWrap: 'anywhere',
              }}
            >
              {d.comment.body}
            </span>
          </button>
        );
      })}
    </div>
  );
});

interface FloatingPill {
  key: number;
  comment: TimelineComment;
  pct: number;
}

/** Pills that drop under the wave as the playhead crosses each comment's
 *  moment (driven by the Rust `comments:show` timeline, toggleable in
 *  settings). */
export function FloatingComments({
  comments,
  durationMs,
  isCurrent,
}: {
  comments: Comment[];
  durationMs: number;
  isCurrent: boolean;
}) {
  const enabled = useSettingsStore((s) => s.floatingComments);
  const [pills, setPills] = useState<FloatingPill[]>([]);
  const seqRef = useRef(0);

  useEffect(() => {
    if (!isCurrent || !enabled) {
      clearCommentsTimeline();
      setPills([]);
      return;
    }
    setCommentsTimeline(
      comments
        .filter((c) => c.timestamp != null && c.body)
        .map((c) => ({
          id: c.id,
          body: c.body,
          timestamp_ms: c.timestamp as number,
          user_avatar_url: c.user?.avatar_url ?? null,
        })),
    );
    return () => {
      clearCommentsTimeline();
      setPills([]);
    };
  }, [isCurrent, enabled, comments]);

  useEffect(() => {
    if (!isCurrent || !enabled || durationMs <= 0) return;
    return subscribeFloatingComments((comment) => {
      const key = (seqRef.current += 1);
      const pct = Math.min(1, Math.max(0, comment.timestamp_ms / durationMs));
      setPills((prev) => [...prev.slice(-2), { key, comment, pct }]);
      setTimeout(() => {
        setPills((prev) => prev.filter((p) => p.key !== key));
      }, 5400);
    });
  }, [isCurrent, enabled, durationMs]);

  if (pills.length === 0) return null;

  return (
    <div className="pointer-events-none absolute inset-0 z-20">
      {pills.map((p) => (
        <div
          key={p.key}
          className="fc-pill absolute top-full mt-1.5"
          style={{ left: `${p.pct * 100}%` }}
        >
          <span
            className="block w-max max-w-[320px] text-left font-medium leading-snug break-words"
            style={{
              background: '#1b1b1f',
              border: '0.5px solid rgba(255,255,255,0.1)',
              borderRadius: 7,
              boxShadow: '0 6px 18px rgba(0,0,0,0.45)',
              color: 'rgba(255,255,255,0.88)',
              fontSize: 11.5,
              padding: '6px 9px',
              overflowWrap: 'anywhere',
            }}
          >
            {p.comment.body}
          </span>
        </div>
      ))}
    </div>
  );
}
