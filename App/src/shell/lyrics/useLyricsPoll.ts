import { useEffect, useState } from 'react';
import { type Lyrics, useSc } from '@sc/data';

const POLL_INTERVAL_MS = 1500;
const MAX_ATTEMPTS = 20; // ~30с — дальше считаем, что лирики не будет

export interface LyricsLoad {
  loading: boolean;
  data: Lyrics | null;
}

/** `/lyrics/{id}` асинхронный: `pending` — индексация в фоне, перезапрашиваем
 *  с интервалом, пока не придёт `found`/`none` либо не кончится лимит попыток.
 *  `enabled=false` (панель закрыта) останавливает поллинг без сброса track-эффекта. */
export function useLyricsPoll(scTrackId: string | null, enabled: boolean): LyricsLoad {
  const sc = useSc();
  const [state, setState] = useState<LyricsLoad>({ loading: false, data: null });

  useEffect(() => {
    if (!scTrackId || !enabled) {
      setState({ loading: false, data: null });
      return;
    }

    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let attempt = 0;
    setState({ loading: true, data: null });

    const tick = () => {
      sc.tracks
        .lyrics(scTrackId)
        .then((data) => {
          if (cancelled) return;
          attempt += 1;
          if (data?.status === 'pending' && attempt < MAX_ATTEMPTS) {
            timer = setTimeout(tick, POLL_INTERVAL_MS);
            return;
          }
          setState({ loading: false, data });
        })
        .catch(() => {
          if (!cancelled) setState({ loading: false, data: null });
        });
    };
    tick();

    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [sc, scTrackId, enabled]);

  return state;
}
