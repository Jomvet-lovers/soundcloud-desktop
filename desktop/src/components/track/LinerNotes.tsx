import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { dateFormatted, durLong, fc } from '../../lib/formatters';
import { ChevronDown, ChevronUp, Hash } from '../../lib/icons';
import type { Track } from '../../stores/player';
import type { TrackAura } from './useTrackAura';

function parseTags(tagList?: string): string[] {
  if (!tagList) return [];
  const tags: string[] = [];
  const re = /"([^"]+)"|(\S+)/g;
  let m: RegExpExecArray | null;
  while ((m = re.exec(tagList))) tags.push(m[1] || m[2]);
  return tags;
}

function Credit({
  label,
  value,
  onClick,
}: {
  label: string;
  value: React.ReactNode;
  onClick?: () => void;
}) {
  return (
    <div className="flex flex-col gap-1 min-w-0">
      <span className="text-[10px] font-medium text-white/30">{label}</span>
      <span
        className={`text-[13px] truncate ${
          onClick
            ? 'text-white/75 hover:text-white cursor-pointer transition-colors'
            : 'text-white/70 selectable'
        }`}
        onClick={onClick}
      >
        {value}
      </span>
    </div>
  );
}

function Stat({ value, label }: { value?: number | null; label: string }) {
  return (
    <span className="whitespace-nowrap">
      <span className="font-semibold text-white/90">{value != null ? fc(value) : '—'}</span>{' '}
      <span className="text-white/40">{label}</span>
    </span>
  );
}

/** The record-sleeve back: glanceable stats, the artist's written notes,
 *  credits-as-typography (album, release, language, ISRC…) and tags. */
export const LinerNotes = React.memo(function LinerNotes({
  track,
}: {
  track: Track;
  aura: TrackAura;
}) {
  const navigate = useNavigate();
  const [expanded, setExpanded] = useState(false);

  const desc = track.description?.trim();
  const descLong = !!desc && desc.length > 280;
  const tags = parseTags(track.tag_list);

  const album = track.enrichment?.album;
  const released = track.release_date
    ? dateFormatted(track.release_date)
    : track.release_year
      ? String(track.release_year)
      : null;
  const full =
    track.full_duration && track.full_duration !== track.duration ? track.full_duration : null;
  const isrc = track.publisher_metadata?.isrc;

  const credits: { label: string; value: React.ReactNode; onClick?: () => void }[] = [];
  if (album?.title)
    credits.push({
      label: 'From album',
      value: album.title,
      onClick: album.id ? () => navigate(`/album/${encodeURIComponent(album.id)}`) : undefined,
    });
  if (released) credits.push({ label: 'Released', value: released });
  if (track.language) credits.push({ label: 'Language', value: track.language });
  if (full) credits.push({ label: 'Full length', value: durLong(full) });
  if (isrc) credits.push({ label: 'ISRC', value: isrc });

  return (
    <section className="flex flex-col gap-6">
      <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-[12.5px]">
        <Stat value={track.playback_count} label={'plays'} />
        <Stat value={track.favoritings_count ?? track.likes_count} label={'likes'} />
        {track.reposts_count != null && <Stat value={track.reposts_count} label={'reposts'} />}
        <Stat value={track.comment_count} label={'Comments'} />
      </div>

      {desc && (
        <div>
          <h3 className="text-[10px] font-medium text-white/30 mb-2.5">{'Description'}</h3>
          <p
            className={`selectable text-[13.5px] text-white/55 leading-relaxed whitespace-pre-wrap break-words ${
              !expanded && descLong ? 'line-clamp-4' : ''
            }`}
          >
            {desc}
          </p>
          {descLong && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="flex items-center gap-1 mt-2 text-[11px] text-white/35 hover:text-white/60 transition-colors cursor-pointer"
            >
              {expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}
              {expanded ? 'Show less' : 'Show more'}
            </button>
          )}
        </div>
      )}

      {credits.length > 0 && (
        <div className="grid grid-cols-2 md:grid-cols-3 gap-x-8 gap-y-4">
          {credits.map((c) => (
            <Credit key={c.label} {...c} />
          ))}
        </div>
      )}

      {tags.length > 0 && (
        <div className="flex items-center gap-1.5 flex-wrap pt-1">
          <Hash size={12} className="text-white/20" />
          {tags.map((tag) => (
            <span
              key={tag}
              className="text-[10px] font-medium text-white/40 hover:text-white/60 transition-colors cursor-default"
            >
              {tag}
            </span>
          ))}
        </div>
      )}
    </section>
  );
});
