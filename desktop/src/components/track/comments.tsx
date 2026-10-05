import React, { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { getCurrentTime, subscribe } from '../../lib/audio';
import { ago, art, durLong } from '../../lib/formatters';
import { type Comment, usePostComment } from '../../lib/hooks';
import { Clock, Loader2, Send } from '../../lib/icons';
import { useAuthStore } from '../../stores/auth';

/** A single voice: who said it, an optional clickable timestamp that seeks,
 *  when it was left, and the body. Flat rows separated by hairlines. */
export const VoiceCard = React.memo(function VoiceCard({
  comment,
  accent,
  onSeek,
}: {
  comment: Comment;
  accent: string;
  onSeek: (seconds: number) => void;
}) {
  const navigate = useNavigate();
  const ts = comment.timestamp;
  const user = comment.user;
  const avatar = art(user?.avatar_url ?? null, 'small');
  const goUser = () => {
    if (user?.urn) navigate(`/user/${encodeURIComponent(user.urn)}`);
  };

  return (
    <div className="flex gap-3 py-3.5 border-b border-white/[0.06]">
      <button type="button" onClick={goUser} className="shrink-0 cursor-pointer self-start">
        {avatar ? (
          <img src={avatar} alt="" loading="lazy" className="w-9 h-9 rounded-full object-cover" />
        ) : (
          <span className="flex w-9 h-9 items-center justify-center rounded-full bg-white/[0.07] text-[13px] font-semibold text-white/45">
            {(user?.username || '?').slice(0, 1).toUpperCase()}
          </span>
        )}
      </button>
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap text-[12.5px]">
          <span
            onClick={goUser}
            className="font-semibold text-white/85 hover:text-white cursor-pointer transition-colors truncate"
          >
            {user?.username ?? 'You'}
          </span>
          {ts != null && (
            <button
              type="button"
              onClick={() => onSeek(ts / 1000)}
              className="font-semibold tabular-nums cursor-pointer transition-colors hover:text-white"
              style={{ color: accent }}
            >
              {durLong(ts)}
            </button>
          )}
          <span className="text-[10.5px] text-white/25">{ago(comment.created_at)}</span>
          {comment.sync === 'pending' && (
            <span className="inline-flex items-center gap-1 text-[10px] text-white/30">
              <Clock size={9} />
              {'sending'}
            </span>
          )}
          {comment.sync === 'failed' && (
            <span className="text-[10px] text-amber-400/85">{'not synced'}</span>
          )}
        </div>
        <p className="selectable text-[13.5px] text-white/70 mt-1 leading-relaxed break-words">
          {comment.body}
        </p>
      </div>
    </div>
  );
});

/** Composer that pins your voice to the current moment — when the track is
 *  playing it shows, live, the timestamp your comment will land on. */
export const CommentForm = React.memo(function CommentForm({
  trackUrn,
  isCurrent,
  accent,
  pendingAt,
  onCommitted,
}: {
  trackUrn: string;
  isCurrent: boolean;
  accent: string;
  /** Position picked on the waveform's lower lane (ms); pinned until posted. */
  pendingAt?: number | null;
  onCommitted?: () => void;
}) {
  const [body, setBody] = useState('');
  const mutation = usePostComment(trackUrn);
  const momentRef = useRef<HTMLSpanElement>(null);
  const myUser = useAuthStore((s) => s.user);
  const myAvatar = myUser?.avatar_url ? art(myUser.avatar_url, 'small') : null;
  const pinned = pendingAt != null;

  useEffect(() => {
    if (!isCurrent || pinned) return;
    const paint = () => {
      const tt = getCurrentTime();
      if (momentRef.current)
        momentRef.current.textContent = durLong(Math.floor(Math.max(0, tt) * 1000));
    };
    paint();
    return subscribe(paint);
  }, [isCurrent, pinned]);

  const submit = () => {
    const text = body.trim();
    if (!text) return;
    const time = getCurrentTime();
    const timestamp =
      pendingAt != null ? pendingAt : time > 0 ? Math.floor(time * 1000) : undefined;
    mutation.mutate({ body: text, timestamp });
    setBody('');
    onCommitted?.();
  };

  const [focused, setFocused] = useState(false);

  return (
    <div
      className="flex-1 min-w-[240px] flex items-center gap-2.5 h-11 pl-3.5 pr-2 rounded-full"
      style={{
        background: 'rgba(255,255,255,0.03)',
        border: `0.5px solid ${focused ? 'var(--color-accent)' : 'rgba(255,255,255,0.12)'}`,
        transition: 'border-color 300ms ease',
      }}
    >
      {myAvatar ? (
        <img src={myAvatar} alt="" className="w-6 h-6 rounded-full object-cover shrink-0" />
      ) : (
        <span className="w-6 h-6 rounded-full bg-white/10 flex items-center justify-center text-[10px] font-semibold text-white/60 shrink-0">
          {(myUser?.username || '?').slice(0, 1).toUpperCase()}
        </span>
      )}
      <input
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onFocus={() => setFocused(true)}
        onBlur={() => setFocused(false)}
        onKeyDown={(e) => {
          if (e.key === 'Enter') {
            e.preventDefault();
            submit();
          }
        }}
        placeholder={'Write a comment...'}
        className="selectable flex-1 min-w-0 bg-transparent text-[13px] text-white/80 placeholder:text-white/25 outline-none"
      />
      {pinned || isCurrent ? (
        <span
          className="inline-flex items-center gap-1 text-[10px] font-semibold shrink-0"
          style={{ color: accent }}
        >
          <Clock size={10} />
          {pendingAt != null ? (
            <span className="tabular-nums">{durLong(pendingAt)}</span>
          ) : (
            <span ref={momentRef} className="tabular-nums">
              0:00
            </span>
          )}
        </span>
      ) : null}
      <button
        type="button"
        onClick={submit}
        disabled={!body.trim() || mutation.isPending}
        className="w-8 h-8 rounded-lg flex items-center justify-center shrink-0 transition-colors duration-200 cursor-pointer disabled:opacity-30 disabled:cursor-default"
        style={{
          color: accent,
          background: body.trim() ? 'rgba(255,255,255,0.08)' : 'transparent',
        }}
      >
        {mutation.isPending ? <Loader2 size={14} className="animate-spin" /> : <Send size={14} />}
      </button>
    </div>
  );
});
