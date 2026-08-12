import { useEffect } from 'react';
import { View } from 'react-native';
import { lyricsUi, useLyricsUi } from '../player/lyrics-ui';
import { scId, usePlayerState } from '../player/PlayerContext';
import { HEADER_HEIGHT, LyricsHeader } from './lyrics/LyricsHeader';
import { LyricsBackdrop } from './lyrics/LyricsBackdrop';
import { LyricsPane } from './lyrics/LyricsPane';
import { LyricsVisualizer } from './lyrics/LyricsVisualizer';
import { SplitDivider } from './lyrics/SplitDivider';
import { TrackColumn } from './lyrics/TrackColumn';
import { useComments } from './lyrics/useComments';
import { useLyricsPoll } from './lyrics/useLyricsPoll';

/** Fullscreen-оверлей лирики (донор `LyricsPanel`): тонкая оболочка — иммерсивный фон +
 *  FFT-визуализатор (опц.) + плавающий хедер + сплит трек/лирика-или-комментарии.
 *  Монтируется в AppShell; открытие — кнопкой в плеере (`lyricsUi.open`). */
export function LyricsScreen() {
  const { open, tab, rightPanelOpen, splitRatio, visualizer } = useLyricsUi();
  const { currentTrack: track } = usePlayerState();
  const lyrics = useLyricsPoll(track ? scId(track) : null, open);
  const comments = useComments(track ? track.id : null, open);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') lyricsUi.close();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [open]);

  if (!open || !track) return null;

  const splitPercent = splitRatio * 100;

  return (
    <View style={{ position: 'absolute', top: 0, left: 0, right: 0, bottom: 0, zIndex: 500, backgroundColor: '#08080a' }}>
      <LyricsBackdrop artworkUrl={track.artwork_url} />
      {visualizer && <LyricsVisualizer />}

      <LyricsHeader
        tab={tab}
        rightPanelOpen={rightPanelOpen}
        visualizer={visualizer}
        onSelectTab={(next) => {
          lyricsUi.setTab(next);
          lyricsUi.setRightPanelOpen(true);
        }}
        onTogglePanel={lyricsUi.toggleRightPanel}
        onToggleVisualizer={lyricsUi.toggleVisualizer}
        onClose={lyricsUi.close}
      />

      {rightPanelOpen ? (
        <View style={{ flex: 1, flexDirection: 'row', zIndex: 10, paddingTop: HEADER_HEIGHT }}>
          <View style={{ width: `${splitPercent}%` }}>
            <TrackColumn track={track} />
          </View>

          <SplitDivider splitRatio={splitRatio} onChange={lyricsUi.setSplitRatio} />

          <View style={{ width: `${100 - splitPercent}%` }}>
            <LyricsPane tab={tab} loading={lyrics.loading} data={lyrics.data} comments={comments} />
          </View>
        </View>
      ) : (
        <View style={{ flex: 1, alignItems: 'center', justifyContent: 'center', zIndex: 10, paddingTop: HEADER_HEIGHT }}>
          <TrackColumn track={track} maxWidth={420} />
        </View>
      )}
    </View>
  );
}
