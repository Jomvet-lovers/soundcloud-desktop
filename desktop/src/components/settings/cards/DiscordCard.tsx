import { MessageCircle } from '../../../lib/icons';
import { type DiscordRpcMode, useSettingsStore } from '../../../stores/settings';
import { Card, Row, Segmented, Toggle } from '../primitives';

const MODES: Array<{ id: DiscordRpcMode; label: string }> = [
  { id: 'track', label: 'Track' },
  { id: 'artist', label: 'Artist' },
  { id: 'activity', label: 'Activity only' },
];

export function DiscordCard() {
  const enabled = useSettingsStore((s) => s.discordRpcEnabled);
  const setEnabled = useSettingsStore((s) => s.setDiscordRpcEnabled);
  const mode = useSettingsStore((s) => s.discordRpcMode);
  const setMode = useSettingsStore((s) => s.setDiscordRpcMode);
  const showButton = useSettingsStore((s) => s.discordRpcShowButton);
  const setShowButton = useSettingsStore((s) => s.setDiscordRpcShowButton);

  return (
    <Card
      title={'Discord Rich Presence'}
      desc={'Show your current SoundCloud playback status in Discord'}
      icon={<MessageCircle size={17} />}
      action={<Toggle checked={enabled} onChange={() => setEnabled(!enabled)} />}
    >
      {enabled ? (
        <div className="space-y-4">
          <div className="space-y-2">
            <p className="text-[12.5px] text-white/50 font-medium">{'Display mode'}</p>
            <Segmented
              value={mode}
              columns={3}
              onChange={setMode}
              options={MODES.map((m) => ({ id: m.id, label: m.label }))}
            />
          </div>
          <Row
            title={'Show GitHub button'}
            desc={'Display the button that opens the GitHub repository'}
          >
            <Toggle checked={showButton} onChange={() => setShowButton(!showButton)} />
          </Row>
        </div>
      ) : null}
    </Card>
  );
}
