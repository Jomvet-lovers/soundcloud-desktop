import * as Popover from '@radix-ui/react-popover';
import React, { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { switchAudioDevice } from '../../lib/audio';
import { trackedInvoke } from '../../lib/diagnostics';
import { Check, speakerIcon16 } from '../../lib/icons';
import { useSettingsStore } from '../../stores/settings';

interface AudioSink {
  name: string;
  description: string;
  /** Hardware/adapter name behind the endpoint (e.g. "AMD High Definition Audio Device"). */
  interface?: string | null;
  is_default: boolean;
}

const ROW =
  'flex w-full items-center gap-3 rounded-lg px-2.5 py-2 text-left transition-colors hover:bg-white/[0.06] disabled:opacity-50 disabled:hover:bg-transparent';

/**
 * Output-device picker anchored in the player bar: the one-click switch while
 * listening. Settings keeps the full card; both go through `switchAudioDevice`,
 * so a pick here persists the same way.
 */
export const AudioOutputPopover = React.memo(function AudioOutputPopover() {
  const follow = useSettingsStore((s) => s.followDefaultOutput);
  const saved = useSettingsStore((s) => s.audioOutputDevice);
  const [open, setOpen] = useState(false);
  const [sinks, setSinks] = useState<AudioSink[]>([]);
  const [switching, setSwitching] = useState(false);

  const refresh = useCallback(() => {
    trackedInvoke<AudioSink[]>('audio_list_devices').then(setSinks).catch(console.error);
  }, []);

  // Devices come and go (Bluetooth, docks), so the list is re-read per open.
  useEffect(() => {
    if (open) refresh();
  }, [open, refresh]);

  const pick = async (name: string | null, label: string | null) => {
    const active = name == null ? follow : !follow && saved === name;
    if (switching) return;
    if (active) {
      setOpen(false);
      return;
    }
    setSwitching(true);
    try {
      // manual = persist the pick; null means follow the system default again.
      await switchAudioDevice(name, true, label);
      toast.success('Audio output switched');
      setOpen(false);
    } catch (err) {
      toast.error(String(err));
    } finally {
      setSwitching(false);
    }
  };

  const defaultSink = sinks.find((s) => s.is_default);
  const current = follow ? null : sinks.find((s) => s.name === saved);
  const label = follow
    ? 'System default'
    : (current?.interface ?? current?.description ?? 'Output device');

  return (
    <Popover.Root open={open} onOpenChange={setOpen}>
      <Popover.Trigger asChild>
        <button
          type="button"
          className="flex size-8 items-center justify-center rounded-full text-white/55 transition-colors hover:bg-white/[0.06] hover:text-white/90"
          title={`Audio output: ${label}`}
          aria-label={'Audio output'}
        >
          {speakerIcon16}
        </button>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Content
          side="top"
          align="end"
          sideOffset={10}
          collisionPadding={12}
          className="z-[200] w-[268px] rounded-xl border border-white/[0.1] bg-[#141417] p-1.5 outline-none"
        >
          <p className="px-2.5 pb-1 pt-1.5 text-[11px] font-medium text-white/45">
            {'Audio output'}
          </p>
          <button
            type="button"
            className={ROW}
            disabled={switching}
            onClick={() => void pick(null, null)}
          >
            <span className="min-w-0 flex-1">
              <span className="block truncate text-[13px] font-medium text-white/85">
                {'System default'}
              </span>
              {follow && defaultSink && (
                <span className="block truncate text-[11px] text-white/40">
                  {defaultSink.interface ?? defaultSink.description}
                </span>
              )}
            </span>
            {follow && <Check size={15} className="shrink-0 text-accent" />}
          </button>
          {sinks.map((sink) => (
            <button
              key={sink.name}
              type="button"
              className={ROW}
              disabled={switching}
              onClick={() => void pick(sink.name, sink.description)}
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[13px] font-medium text-white/85">
                  {sink.interface ?? sink.description}
                </span>
                {sink.interface && (
                  <span className="block truncate text-[11px] text-white/40">
                    {sink.description}
                  </span>
                )}
              </span>
              {!follow && sink.name === saved && (
                <Check size={15} className="shrink-0 text-accent" />
              )}
            </button>
          ))}
          {sinks.length === 0 && (
            <p className="px-2.5 py-3 text-[12px] text-white/35">{'No output devices found'}</p>
          )}
        </Popover.Content>
      </Popover.Portal>
    </Popover.Root>
  );
});
