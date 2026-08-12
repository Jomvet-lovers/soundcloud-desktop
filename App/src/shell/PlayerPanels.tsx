import { usePanels } from './panels';
import { EqualizerPanel } from './EqualizerPanel';
import { QueuePanel } from './QueuePanel';

/** Оверлейные панели плеера над контентом (открывает NPB через usePanels). */
export function PlayerPanels() {
  const panels = usePanels();
  return (
    <>
      <QueuePanel open={panels.isOpen('queue')} onClose={panels.close} />
      <EqualizerPanel open={panels.isOpen('eq')} onClose={panels.close} />
    </>
  );
}
