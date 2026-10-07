import { useCallback, useEffect, useState } from 'react';
import { toast } from 'sonner';
import { switchAudioDevice } from '../../../lib/audio';
import { trackedInvoke } from '../../../lib/diagnostics';
import { Check, Volume2 } from '../../../lib/icons';
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
  const [following, setFollowing] = useState(true);
  const [switching, setSwitching] = useState(false);

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

  const handleSwitch = async (sinkName: string | null) => {
    const current = sinks.find((s) => s.is_default);
    const alreadyActive = sinkName == null ? following : !following && current?.name === sinkName;
    if (switching || alreadyActive) return;
    setSwitching(true);
    try {
      await switchAudioDevice(sinkName, true);
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
          onClick={() => handleSwitch(null)}
          disabled={switching}
          className={rowCls}
        >
          <span className="min-w-0">
            <span className={titleCls}>{'Windows default'}</span>
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
            onClick={() => handleSwitch(sink.name)}
            disabled={switching}
            className={`${rowCls} border-t border-white/[0.05]`}
          >
            <span className="min-w-0">
              <span className={titleCls}>{sink.interface ?? sink.description}</span>
              {sink.interface && <span className={subCls}>{sink.description}</span>}
            </span>
            {!following && sink.is_default && <Check size={15} className="shrink-0 text-accent" />}
          </button>
        ))}
      </div>
    </Card>
  );
}
