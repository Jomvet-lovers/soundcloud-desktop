import { NavHome } from '../../../lib/nav-icons';
import { type StartupPage, useSettingsStore } from '../../../stores/settings';
import { Card, Segmented } from '../primitives';

const PAGES: Array<{ id: StartupPage; label: string }> = [
  { id: 'home', label: 'Home' },
  { id: 'feed', label: 'Feed' },
  { id: 'search', label: 'Search' },
  { id: 'library', label: 'Library' },
  { id: 'settings', label: 'Settings' },
];

export function StartupCard() {
  const startupPage = useSettingsStore((s) => s.startupPage);
  const setStartupPage = useSettingsStore((s) => s.setStartupPage);

  return (
    <Card
      title={'Startup'}
      desc={'Choose which page opens when the app launches'}
      icon={<NavHome size={17} />}
    >
      <Segmented
        value={startupPage}
        columns={5}
        onChange={setStartupPage}
        options={PAGES.map((p) => ({ id: p.id, label: p.label }))}
      />
    </Card>
  );
}
