import { Headphones } from '../../../lib/icons';
import { useSettingsStore } from '../../../stores/settings';
import { Card, Row, Toggle } from '../primitives';

export function PlaybackCard() {
  const floatingComments = useSettingsStore((s) => s.floatingComments);
  const setFloatingComments = useSettingsStore((s) => s.setFloatingComments);
  const normalizeVolume = useSettingsStore((s) => s.normalizeVolume);
  const setNormalizeVolume = useSettingsStore((s) => s.setNormalizeVolume);
  const highQualityStreaming = useSettingsStore((s) => s.highQualityStreaming);
  const setHighQualityStreaming = useSettingsStore((s) => s.setHighQualityStreaming);

  return (
    <Card title={'Playback'} icon={<Headphones size={17} />}>
      <div className="divide-y divide-white/[0.05]">
        <Row title={'Floating comments'} desc={'Show comments as floating pills during playback'}>
          <Toggle
            checked={floatingComments}
            onChange={() => setFloatingComments(!floatingComments)}
          />
        </Row>
        <Row
          title={'Volume normalization'}
          desc={'Balances quiet and loud tracks to a more even level'}
        >
          <Toggle checked={normalizeVolume} onChange={() => setNormalizeVolume(!normalizeVolume)} />
        </Row>
        <Row
          title={'High quality streaming'}
          desc={'Prefer the highest available quality during playback'}
        >
          <Toggle
            checked={highQualityStreaming}
            onChange={() => setHighQualityStreaming(!highQualityStreaming)}
          />
        </Row>
      </div>
    </Card>
  );
}
