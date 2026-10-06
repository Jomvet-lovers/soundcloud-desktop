import * as Popover from '@radix-ui/react-popover';
import { useQueryClient } from '@tanstack/react-query';
import React, { useEffect, useState } from 'react';
import { toast } from 'sonner';
import { api } from '../../lib/api';
import { type DownloadFormat, downloadTrack } from '../../lib/cache';
import { fc } from '../../lib/formatters';
import { invalidateAllLikesCache } from '../../lib/hooks';
import { Check, Download, Heart, LinkIcon, Loader2 } from '../../lib/icons';
import { optimisticToggleLike, setLikedUrn, useLiked } from '../../lib/likes';
import { usePulseHeart } from '../../lib/pulse-heart';
import { getTrackDisplay } from '../../lib/track-display';
import type { Track } from '../../stores/player';

/** Accent like-chip: icon + count in an outlined pill, accent-tinted when active. */
const EngagementChip = React.memo(function EngagementChip({
  active,
  icon,
  count,
  label,
  onClick,
  pillRef,
  heartRef,
}: {
  active: boolean;
  icon: React.ReactNode;
  count: number;
  label: string;
  onClick: () => void;
  pillRef?: React.Ref<HTMLButtonElement>;
  heartRef?: React.Ref<HTMLSpanElement>;
}) {
  return (
    <button
      ref={pillRef}
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={`inline-flex items-center gap-1.5 h-10 px-3.5 rounded-full border text-[12.5px] font-medium tabular-nums transition-colors cursor-pointer ${
        active
          ? 'text-accent border-accent/45'
          : 'text-white/65 border-white/[0.14] hover:border-white/[0.32] hover:text-white'
      }`}
    >
      <span ref={heartRef} className="flex items-center justify-center">
        {icon}
      </span>
      <span>{fc(count)}</span>
    </button>
  );
});

export const LikeBtn = React.memo(({ trackUrn, count }: { trackUrn: string; count?: number }) => {
  const liked = useLiked(trackUrn);
  const { shownLiked, pulse, heartRef, pillRef } = usePulseHeart(liked);
  const [localCount, setLocalCount] = useState(count ?? 0);
  const qc = useQueryClient();

  useEffect(() => setLocalCount(count ?? 0), [count]);

  const toggle = async () => {
    const next = !liked;
    pulse(next);
    setLocalCount((c) => c + (next ? 1 : -1));
    const cached = qc.getQueryData<Track>(['track', trackUrn]);
    if (cached) optimisticToggleLike(qc, cached, next);
    else setLikedUrn(trackUrn, next);
    invalidateAllLikesCache();
    try {
      await api(`/likes/tracks/${encodeURIComponent(trackUrn)}`, {
        method: next ? 'POST' : 'DELETE',
      });
      qc.invalidateQueries({ queryKey: ['track', trackUrn, 'favoriters'] });
    } catch {
      setLocalCount((c) => c + (next ? -1 : 1));
      if (cached) optimisticToggleLike(qc, cached, !next);
      else setLikedUrn(trackUrn, !next);
    }
  };

  return (
    <EngagementChip
      active={shownLiked}
      icon={<Heart size={15} fill={shownLiked ? 'currentColor' : 'none'} />}
      count={localCount}
      label={'likes'}
      onClick={toggle}
      pillRef={pillRef as React.RefObject<HTMLButtonElement | null>}
      heartRef={heartRef as React.RefObject<HTMLSpanElement | null>}
    />
  );
});

/** Icon-only button for the utility rail. */
export const IconAction = React.memo(function IconAction({
  icon,
  label,
  onClick,
  active,
}: {
  icon: React.ReactNode;
  label: string;
  onClick?: () => void;
  active?: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={`inline-flex items-center justify-center w-10 h-10 rounded-xl transition-all duration-200 ease-[var(--ease-apple)] cursor-pointer ${
        active
          ? 'text-accent bg-accent/15'
          : 'text-white/60 hover:text-white/95 hover:bg-white/[0.07]'
      }`}
    >
      {icon}
    </button>
  );
});

export const CopyIconAction = React.memo(function CopyIconAction({ url }: { url?: string }) {
  const [copied, setCopied] = useState(false);

  if (!url) return null;

  const copy = () => {
    try {
      const u = new URL(url);
      for (const p of ['utm_medium', 'utm_campaign', 'utm_source']) u.searchParams.delete(p);
      navigator.clipboard.writeText(u.toString().replace(/\?$/, ''));
    } catch {
      navigator.clipboard.writeText(url);
    }
    setCopied(true);
    setTimeout(() => setCopied(false), 1600);
  };

  return (
    <button
      type="button"
      onClick={copy}
      title={copied ? 'Copied!' : 'Copy link'}
      aria-label={copied ? 'Copied!' : 'Copy link'}
      className={`inline-flex items-center justify-center w-10 h-10 rounded-full border transition-colors cursor-pointer ${
        copied
          ? 'text-emerald-400 border-emerald-400/45'
          : 'text-white/60 border-white/[0.14] hover:border-white/[0.32] hover:text-white'
      }`}
    >
      {copied ? <Check size={16} /> : <LinkIcon size={16} />}
    </button>
  );
});

const DOWNLOAD_FORMATS: Array<{ id: DownloadFormat; label: string; desc: string }> = [
  { id: 'm4a', label: 'M4A', desc: 'AAC' },
  { id: 'mp3', label: 'MP3', desc: '320 kbps' },
  { id: 'flac', label: 'FLAC', desc: 'Lossless' },
  { id: 'wav', label: 'WAV', desc: 'Uncompressed' },
];

export const DownloadButton = React.memo(({ track }: { track: Track }) => {
  const [loading, setLoading] = useState(false);
  const [open, setOpen] = useState(false);

  const download = async (format: DownloadFormat) => {
    if (loading) return;
    setOpen(false);
    setLoading(true);
    try {
      const display = getTrackDisplay(track);
      await downloadTrack(track.urn, display.artistLine || track.user.username, display.title, {
        artworkUrl: track.artwork_url,
        durationMs: track.duration,
        format,
      });
      toast.success('Saved');
    } catch (e: unknown) {
      if (e instanceof Error && e.message === 'cancelled') return;
      toast.error(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          disabled={loading}
          title={'Download'}
          aria-label={'Download'}
          className="inline-flex items-center justify-center w-10 h-10 rounded-full border border-white/[0.14] text-white/60 hover:text-white hover:border-white/[0.32] transition-colors cursor-pointer disabled:opacity-50"
        >
          {loading ? <Loader2 size={16} className="animate-spin" /> : <Download size={16} />}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          side="top"
          align="center"
          sideOffset={8}
          collisionPadding={12}
          className="z-[200] w-[190px] rounded-xl border border-white/[0.1] bg-[#141417] p-1.5 outline-none"
        >
          {DOWNLOAD_FORMATS.map((f) => (
            <button
              key={f.id}
              type="button"
              onClick={() => void download(f.id)}
              className="flex w-full items-center justify-between gap-3 rounded-lg px-3 py-2 text-left transition-colors hover:bg-white/[0.06] cursor-pointer"
            >
              <span className="text-[12.5px] font-medium text-white/85">{f.label}</span>
              <span className="text-[10.5px] text-white/35">{f.desc}</span>
            </button>
          ))}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
});
