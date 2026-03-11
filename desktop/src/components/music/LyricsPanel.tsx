import { Loader2, Music, X } from '../../lib/icons';
import React, { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { searchLyrics } from '../../lib/lyrics';
import type { Track } from '../../stores/player';

interface LyricsPanelProps {
  track: Track | null;
  open: boolean;
  onClose: () => void;
}

export const LyricsPanel = React.memo(({ track, open, onClose }: LyricsPanelProps) => {
  const { t } = useTranslation();
  const [lyrics, setLyrics] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    if (!track || !open) {
      setLyrics(null);
      setError(false);
      return;
    }

    setLoading(true);
    setError(false);

    const timeoutId = setTimeout(() => {
      setError(true);
      setLoading(false);
    }, 12000);

    searchLyrics(track.user.username, track.title)
      .then((result) => {
        clearTimeout(timeoutId);
        setLyrics(result);
        if (!result) setError(true);
      })
      .catch(() => {
        clearTimeout(timeoutId);
        setError(true);
      })
      .finally(() => setLoading(false));

    return () => clearTimeout(timeoutId);
  }, [track?.urn, open]);

  if (!track) return null;

  return (
    <>
      {/* Backdrop */}
      <div
        className={`fixed inset-0 bg-black/40 backdrop-blur-sm z-40 transition-opacity duration-300 ${
          open ? 'opacity-100' : 'opacity-0 pointer-events-none'
        }`}
        onClick={onClose}
      />

      {/* Panel */}
      <div
        className={`fixed top-0 right-0 bottom-0 w-[420px] z-50 flex flex-col transition-transform duration-300 ease-[var(--ease-apple)] ${
          open ? 'translate-x-0' : 'translate-x-full'
        }`}
        style={{
          background: 'rgba(18, 18, 20, 0.92)',
          backdropFilter: 'blur(60px) saturate(1.8)',
          borderLeft: '1px solid rgba(255,255,255,0.06)',
        }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-5 pt-5 pb-3 border-b border-white/[0.06]">
          <div className="flex items-center gap-3">
            <Music size={18} className="text-white/40" />
            <h2 className="text-base font-semibold tracking-tight">{t('track.lyrics')}</h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="w-7 h-7 rounded-lg flex items-center justify-center text-white/30 hover:text-white/60 hover:bg-white/[0.06] transition-all duration-150 cursor-pointer"
          >
            <X size={16} />
          </button>
        </div>

        {/* Track info */}
        <div className="px-5 py-4 border-b border-white/[0.04]">
          <p className="text-[13px] font-medium text-white/90 truncate">{track.title}</p>
          <p className="text-[11px] text-white/40 truncate mt-0.5">{track.user.username}</p>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto px-5 py-6">
          {loading ? (
            <div className="flex flex-col items-center justify-center h-full gap-3">
              <Loader2 size={24} className="animate-spin text-white/20" />
              <p className="text-[12px] text-white/30">{t('track.lyricsLoading')}</p>
            </div>
          ) : error || !lyrics ? (
            <div className="flex flex-col items-center justify-center h-full gap-3 text-center px-6">
              <Music size={32} className="text-white/10" />
              <p className="text-[13px] text-white/40">{t('track.lyricsNotFound')}</p>
              <p className="text-[11px] text-white/20 leading-relaxed">
                We couldn't find lyrics for this track. Try searching on Genius.com
              </p>
            </div>
          ) : (
            <div className="text-[14px] text-white/70 leading-loose whitespace-pre-wrap">
              {lyrics}
            </div>
          )}
        </div>
      </div>
    </>
  );
});
