import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { switchAudioDevice } from '../../../lib/audio';
import { trackedInvoke } from '../../../lib/diagnostics';
import { Check, Volume2 } from '../../../lib/icons';
import { useSettingsStore } from '../../../stores/settings';
import { Card } from '../primitives';

interface AudioSink {
  name: string;
  description: string;
  /** Hardware/adapter name behind the endpoint (e.g. "AMD High Definition Audio Device"). */
  interface?: string | null;
  is_default: boolean;
}

/** Flat two-line device list — adapter name on top, endpoint name below. */
export function AudioDeviceCard() {
  const [sinks, setSinks] = useState<AudioSink[]>([]);
  const [following, setFollowing] = useState(() => useSettingsStore.getState().followDefaultOutput);
  // The active sink is the saved choice, not `is_default`: that flag is the OS
  // default and stays true on its row after a manual pick.
  const savedDevice = useSettingsStore((s) => s.audioOutputDevice);
  const [switching, setSwitching] = useState(false);

  // Rust owns the runtime truth, but the persisted choice is what a fresh
  // mount should show before the device query lands.
  useEffect(
    () =>
      useSettingsStore.subscribe((state, prev) => {
        if (state.followDefaultOutput !== prev.followDefaultOutput) {
          setFollowing(state.followDefaultOutput);
        }
      }),
    [],
  );

  const refreshSinks = useCallback(() => {
    trackedInvoke<AudioSink[]>('audio_list_devices').then(setSinks).catch(console.error);
    trackedInvoke<boolean>('audio_get_follow_default_output')
      .then(setFollowing)
      .catch(console.error);
  }, []);

  useEffect(() => {
    refreshSinks();
    const onFocus = () => refreshSinks();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [refreshSinks]);

  const handleSwitch = async (sinkName: string | null, label: string | null) => {
    const alreadyActive = sinkName == null ? following : !following && savedDevice === sinkName;
    if (switching || alreadyActive) return;
    setSwitching(true);
    try {
      // The label is the fallback matcher when the OS re-ids the endpoint later
      // (re-paired Bluetooth); Rust tries the saved id first.
      await switchAudioDevice(sinkName, true, label);
      setFollowing(sinkName == null);
      if (sinkName != null) {
        setSinks((prev) => prev.map((s) => ({ ...s, is_default: s.name === sinkName })));
      }
      toast.success('Audio device switched');
    } catch (err) {
      toast.error(String(err));
    } finally {
      setSwitching(false);
    }
  };

  if (sinks.length === 0) return null;

  const defaultSink = sinks.find((s) => s.is_default);
  const rowCls =
    'group flex items-center justify-between gap-3 py-2.5 text-left cursor-pointer disabled:opacity-50';
  const titleCls =
    'block truncate text-[13px] font-semibold text-white/85 transition-colors group-hover:text-white';
  const subCls = 'block truncate text-[11.5px] text-white/40';

  return (
    <Card title={'Audio output'} icon={<Volume2 size={17} />}>
      <div className="flex flex-col">
        <button
          type="button"
          onClick={() => handleSwitch(null, null)}
          disabled={switching}
          className={rowCls}
        >
          <span className="min-w-0">
            <span className={titleCls}>{'System default'}</span>
            {defaultSink && (
              <span className={subCls}>{defaultSink.interface ?? defaultSink.description}</span>
            )}
          </span>
          {following && <Check size={15} className="shrink-0 text-accent" />}
        </button>

        {sinks.map((sink) => (
          <button
            key={sink.name}
            type="button"
            onClick={() => handleSwitch(sink.name, sink.description)}
            disabled={switching}
            className={`${rowCls} border-t border-white/[0.05]`}
          >
            <span className="min-w-0">
              <span className={titleCls}>{sink.interface ?? sink.description}</span>
              {sink.interface && <span className={subCls}>{sink.description}</span>}
            </span>
            {!following && savedDevice === sink.name && (
              <Check size={15} className="shrink-0 text-accent" />
            )}
          </button>
        ))}
      </div>
    </Card>
  );
}
