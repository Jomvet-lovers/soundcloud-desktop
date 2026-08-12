import { View } from 'react-native';
import { AudioLinesIcon, MicVocalIcon, QueueIcon, ScText, VolumeIcon } from '@sc/ui';
import { useT } from '../../i18n';
import { useEq } from '../../player/EqContext';
import { lyricsUi, useLyricsUi } from '../../player/lyrics-ui';
import { VOLUME_MAX } from '../../player/PlayerContext';
import { usePanels } from '../panels';
import { IconButton } from './IconButton';
import { Slider } from './Slider';
import { TuningBtn } from './TuningBtn';

/** Правый кластер плеера (донор `.npb-row`): настройка звука, EQ, лирика, очередь,
 *  громкость. Громкость 0-200% (>100 — буст, янтарь). EQ/очередь — панели,
 *  лирика — fullscreen-оверлей. */
export function Extras({
  volume,
  onSetVolume,
  onToggleMute,
  accentColor,
  glowColor,
}: {
  volume: number;
  onSetVolume: (v: number) => void;
  onToggleMute: () => void;
  accentColor: string;
  glowColor: string;
}) {
  const idle = 'rgba(255,255,255,0.55)';
  const panels = usePanels();
  const eq = useEq();
  const lyrics = useLyricsUi();
  const boosted = volume > 100;
  const t = useT();

  return (
    <View style={{ flexDirection: 'row', alignItems: 'center', gap: 2 }}>
      <TuningBtn />
      <IconButton size={30} onPress={() => panels.toggle('eq')} tooltip={t('player.eq')}>
        <AudioLinesIcon size={16} color={eq.enabled ? accentColor : idle} />
      </IconButton>
      <IconButton size={30} onPress={() => lyricsUi.toggle()} tooltip={t('player.lyrics')}>
        <MicVocalIcon size={16} color={lyrics.open ? accentColor : idle} />
      </IconButton>
      <IconButton size={30} onPress={() => panels.toggle('queue')} tooltip={t('player.queue')}>
        <QueueIcon size={16} color={panels.isOpen('queue') ? accentColor : idle} />
      </IconButton>
      <IconButton size={36} onPress={onToggleMute} tooltip={t('player.mute')}>
        <VolumeIcon size={16} color={volume === 0 ? accentColor : idle} muted={volume === 0} low={volume < 50} />
      </IconButton>
      <View style={{ flexDirection: 'row', alignItems: 'center', gap: 8, paddingLeft: 4 }}>
        <Slider
          value={volume / VOLUME_MAX}
          onSeek={(f) => onSetVolume(f * VOLUME_MAX)}
          color={volume === 0 ? idle : boosted ? '#fbbf24' : 'rgba(255,255,255,0.6)'}
          glowColor={glowColor}
          height={3}
          hoverHeight={4}
          thumbSize={10}
          tickFrac={0.5}
          style={{ width: 72 }}
        />
        <ScText style={{ fontSize: 10, width: 30, textAlign: 'right', color: boosted ? 'rgba(251,191,36,0.7)' : 'rgba(255,255,255,0.3)' }}>
          {volume}%
        </ScText>
      </View>
    </View>
  );
}
