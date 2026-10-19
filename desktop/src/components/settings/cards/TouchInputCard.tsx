import { Hand } from '../../../lib/icons';
import { useSettingsStore } from '../../../stores/settings';
import { Card, Row, Toggle } from '../primitives';

export function TouchInputCard() {
  const touchKeyboardAutoShow = useSettingsStore((s) => s.touchKeyboardAutoShow);
  const setTouchKeyboardAutoShow = useSettingsStore((s) => s.setTouchKeyboardAutoShow);

  return (
    <Card title={'Touch'} desc={'Touch panel and pen display options'} icon={<Hand size={17} />}>
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
