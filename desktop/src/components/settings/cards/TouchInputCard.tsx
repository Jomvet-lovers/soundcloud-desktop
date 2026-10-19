import { Hand } from '../../../lib/icons';
import { useSettingsStore } from '../../../stores/settings';
import { Card, Row, Segmented, Toggle } from '../primitives';

export function TouchInputCard() {
  const inputMode = useSettingsStore((s) => s.inputMode);
  const setInputMode = useSettingsStore((s) => s.setInputMode);
  const touchKeyboardAutoShow = useSettingsStore((s) => s.touchKeyboardAutoShow);
  const setTouchKeyboardAutoShow = useSettingsStore((s) => s.setTouchKeyboardAutoShow);

  return (
    <Card title={'Touch'} desc={'Touch panel and pen display options'} icon={<Hand size={17} />}>
      <Row
        title={'Input mode'}
        desc={
          'Auto follows real touch hardware. Use Touch when touch arrives as mouse input (SlothMon, Spacedesk, Moonlight and other virtual displays)'
        }
      >
        <Segmented
          value={inputMode}
          columns={2}
          onChange={setInputMode}
          options={[
            { id: 'auto', label: 'Auto' },
            { id: 'touch', label: 'Touch' },
          ]}
        />
      </Row>
      <Row
        title={'Show touch keyboard automatically'}
        desc={
          'Pop up the Windows touch keyboard when a text field is tapped in touch mode (off by default)'
        }
      >
        <Toggle
          checked={touchKeyboardAutoShow}
          onChange={() => setTouchKeyboardAutoShow(!touchKeyboardAutoShow)}
        />
      </Row>
    </Card>
  );
}
