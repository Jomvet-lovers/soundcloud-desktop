import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { type Track, useSc } from '@sc/data';
import { beginLoad, reportDownload } from './download';
import { onUnlike } from './likes';
import { getPositionSecs, resetPosition, setDuration, setPosition } from './position';

export type RepeatMode = 'off' | 'all' | 'one';
export type PitchMode = 'auto' | 'manual';

/** A-B «лучший фрагмент»: границы в source-секундах; `b === null` — точка A задана, ждём B. */
export interface AbLoop {
  a: number;
  b: number | null;
}

export const PLAYBACK_RATE_MIN = 0.5;
export const PLAYBACK_RATE_MAX = 2.0;
export const PLAYBACK_RATE_STEP = 0.05;
export const PITCH_SEMITONES_MIN = -12;
export const PITCH_SEMITONES_MAX = 12;
export const PITCH_SEMITONES_STEP = 0.5;
/** Минимальная ширина петли / зазор между ручками, сек. */
export const AB_MIN_GAP = 0.2;
export const VOLUME_MAX = 200;

const clampRate = (r: number) =>
  !Number.isFinite(r) ? 1 : Math.round(Math.max(PLAYBACK_RATE_MIN, Math.min(PLAYBACK_RATE_MAX, r)) * 100) / 100;
const clampPitch = (s: number) =>
  !Number.isFinite(s) ? 0 : Math.round(Math.max(PITCH_SEMITONES_MIN, Math.min(PITCH_SEMITONES_MAX, s)) * 2) / 2;

/** Эффективная тональность: в auto — эквивалент скорости (rate↔pitch связаны), в manual — слайдер. */
export function effectivePitch(rate: number, mode: PitchMode, manual: number): number {
  if (mode === 'auto') return clampPitch((Math.log(Math.max(0.01, rate)) / Math.log(2)) * 12);
  return clampPitch(manual);
}

/** Числовой sc-id из urn (`soundcloud:tracks:123` → `123`). */
export const scId = (track: Track): string => track.id.split(':').pop() ?? track.id;

export interface PlayerState {
  queue: Track[];
  index: number;
  currentTrack: Track | null;
  playing: boolean;
  volume: number;
  volumeBeforeMute: number;
  shuffle: boolean;
  repeat: RepeatMode;
  abLoop: AbLoop | null;
  playbackRate: number;
  pitchSemitones: number;
  pitchMode: PitchMode;
  playQueue: (tracks: Track[], startIndex?: number, startAt?: number) => void;
  /** `startAt` — стартовая позиция в секундах (клик по волне/комменту у неиграющего трека). */
  toggle: (track: Track, queueIfNew?: Track[], startAt?: number) => void;
  togglePlayPause: () => void;
  next: () => void;
  prev: () => void;
  toggleShuffle: () => void;
  cycleRepeat: () => void;
  seek: (secs: number) => void;
  setVolume: (v: number) => void;
  toggleMute: () => void;
  setPlaybackRate: (rate: number) => void;
  resetPlaybackRate: () => void;
  setPitchSemitones: (value: number) => void;
  resetPitchSemitones: () => void;
  setPitchMode: (mode: PitchMode) => void;
  cycleAbPoint: (pos: number) => void;
  nudgeAbBound: (which: 'a' | 'b', value: number) => void;
  clearAbLoop: () => void;
  /** дозалить хвост очереди, не трогая текущий трек/позицию */
  extendQueue: (tracks: Track[]) => void;
  /** вставить трек сразу после текущего */
  playNext: (track: Track) => void;
  /** перетащить элемент очереди (индекс→индекс), текущий трек не сбивается */
  reorderQueue: (from: number, to: number) => void;
  /** убрать трек из очереди по id */
  removeFromQueue: (id: string) => void;
}

const PlayerCtx = createContext<PlayerState | null>(null);

export const usePlayerState = (): PlayerState => {
  const ctx = useContext(PlayerCtx);
  if (!ctx) throw new Error('PlayerProvider отсутствует выше по дереву');
  return ctx;
};

/**
 * Очередь/шаффл/повтор/тюнинг — фронтовая ответственность (в sc-core Queue-модель
 * заведена, но не используется — см. project_queue_continuation_source). Ядро знает
 * «играй этот urn», позицию, ended, и принимает speed/eq/ab-loop/volume.
 *
 * Позиция/прогресс НЕ в стейте (см. position.ts, императивно). Тюнинг (скорость,
 * pitch, ab-loop, громкость) — низкочастотный, живёт в стейте штатно.
 */
export function PlayerProvider({ children }: { children: ReactNode }) {
  const sc = useSc();
  const [queue, setQueue] = useState<Track[]>([]);
  const [index, setIndex] = useState(0);
  const [playing, setPlaying] = useState(false);
  const [volume, setVolumeState] = useState(50);
  const [volumeBeforeMute, setVolumeBeforeMute] = useState(50);
  const [shuffle, setShuffle] = useState(false);
  const [repeat, setRepeat] = useState<RepeatMode>('off');
  const [abLoop, setAbLoop] = useState<AbLoop | null>(null);
  const [playbackRate, setRate] = useState(1);
  const [pitchSemitones, setPitch] = useState(0);
  const [pitchMode, setPitchModeState] = useState<PitchMode>('auto');
  const orderRef = useRef<Track[]>([]);

  const currentTrack = queue[index] ?? null;

  // Актуальные queue/index/rate/volume для мутаций без stale-замыканий.
  const queueRef = useRef(queue);
  queueRef.current = queue;
  const indexRef = useRef(index);
  indexRef.current = index;
  const rateRef = useRef(playbackRate);
  rateRef.current = playbackRate;
  const volumeRef = useRef(volume);
  volumeRef.current = volume;
  const repeatRef = useRef(repeat);
  repeatRef.current = repeat;
  // Restore: ядро ещё не грузило восстановленный трек — первый play загрузит+сикнет.
  const loadedRef = useRef(false);
  const restoredPosRef = useRef(0);

  useEffect(() => {
    const offPlayback = sc.on('playback', (ev) => {
      if (ev.kind === 'ended') {
        setPlaying(false);
        advance();
      }
    });
    const offPos = sc.on('position', (secs) => setPosition(secs));
    const offDl = sc.on('download_progress', (ev) => reportDownload(ev.urn, ev.fraction));
    return () => {
      offPlayback();
      offPos();
      offDl();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sc]);

  const playAt = useCallback(
    (list: Track[], i: number, startAt = 0) => {
      const track = list[i];
      if (!track) return;
      loadedRef.current = true;
      resetPosition();
      setDuration(track.duration_ms);
      beginLoad(track.id);
      setAbLoop(null);
      void sc.player.setAbLoop(null, null);
      setPlaying(true);
      void sc.player
        .play(track.id)
        .then(() => {
          // Новый player-источник сбрасывает скорость/громкость — переприменяем.
          if (rateRef.current !== 1) void sc.player.setSpeed(rateRef.current);
          void sc.player.setVolume(volumeRef.current / 100);
          if (startAt > 0.5) {
            setPosition(startAt);
            void sc.player.seek(startAt);
          }
        })
        .catch(() => setPlaying(false));
    },
    [sc],
  );

  const playQueue = useCallback(
    (tracks: Track[], startIndex = 0, startAt = 0) => {
      orderRef.current = tracks;
      const ordered = shuffle ? shuffleKeepingFirst(tracks, startIndex) : tracks;
      setQueue(ordered);
      const newIndex = shuffle ? 0 : startIndex;
      setIndex(newIndex);
      playAt(ordered, newIndex, startAt);
    },
    [shuffle, playAt],
  );

  const toggle = useCallback(
    (track: Track, queueIfNew?: Track[], startAt = 0) => {
      if (currentTrack?.id === track.id) {
        // Восстановленный трек ещё не в ядре — грузим и сикаем на сохранённую позицию.
        if (!loadedRef.current) {
          playAt(queueRef.current, indexRef.current, restoredPosRef.current);
          return;
        }
        if (playing) {
          void sc.player.pause();
          setPlaying(false);
        } else {
          void sc.player.resume();
          setPlaying(true);
        }
        return;
      }
      const list = queueIfNew ?? [track];
      const i = list.findIndex((t) => t.id === track.id);
      playQueue(list, i >= 0 ? i : 0, startAt);
    },
    [currentTrack, playing, sc, playQueue, playAt],
  );

  const togglePlayPause = useCallback(() => {
    if (!currentTrack) return;
    toggle(currentTrack);
  }, [currentTrack, toggle]);

  // Читаем очередь/повтор через refs — `advance` дёргается из раз-подписанного
  // обработчика `ended`, замыкание на первый рендер иначе видело бы пустую очередь.
  function advance() {
    const q = queueRef.current;
    const rep = repeatRef.current;
    setIndex((i) => {
      if (rep === 'one') {
        playAt(q, i);
        return i;
      }
      const nextI = i + 1;
      if (nextI < q.length) {
        playAt(q, nextI);
        return nextI;
      }
      if (rep === 'all' && q.length > 0) {
        playAt(q, 0);
        return 0;
      }
      return i;
    });
  }

  const next = useCallback(() => advance(), []); // eslint-disable-line react-hooks/exhaustive-deps
  // Донор `handlePrev`: >3с — в начало, иначе предыдущий трек.
  const prev = useCallback(() => {
    if (getPositionSecs() > 3) {
      setPosition(0);
      void sc.player.seek(0);
      return;
    }
    setIndex((i) => {
      const p = i > 0 ? i - 1 : repeat === 'all' ? queue.length - 1 : 0;
      playAt(queue, p);
      return p;
    });
  }, [queue, repeat, playAt, sc]);

  const toggleShuffle = useCallback(() => {
    setShuffle((s) => {
      const willShuffle = !s;
      const base = orderRef.current.length ? orderRef.current : queue;
      const cur = queue[index];
      const curIdx = cur ? base.findIndex((t) => t.id === cur.id) : 0;
      const reordered = willShuffle ? shuffleKeepingFirst(base, curIdx) : base;
      setQueue(reordered);
      setIndex(willShuffle ? 0 : Math.max(0, curIdx));
      return willShuffle;
    });
  }, [queue, index]);

  const cycleRepeat = useCallback(() => {
    setRepeat((r) => (r === 'off' ? 'all' : r === 'all' ? 'one' : 'off'));
  }, []);

  const seek = useCallback(
    (secs: number) => {
      if (!Number.isFinite(secs)) return;
      const s = Math.max(0, secs);
      setPosition(s);
      void sc.player.seek(s);
    },
    [sc],
  );

  const setVolume = useCallback(
    (v: number) => {
      const clamped = Math.round(Math.max(0, Math.min(VOLUME_MAX, v)));
      setVolumeState((prev) => {
        if (clamped === 0 && prev > 0) setVolumeBeforeMute(prev);
        return clamped;
      });
      void sc.player.setVolume(clamped / 100);
    },
    [sc],
  );

  const toggleMute = useCallback(() => {
    setVolume(volumeRef.current > 0 ? 0 : volumeBeforeMute || 50);
  }, [setVolume, volumeBeforeMute]);

  const setPlaybackRate = useCallback(
    (rate: number) => {
      const r = clampRate(rate);
      setRate(r);
      void sc.player.setSpeed(r);
    },
    [sc],
  );
  const resetPlaybackRate = useCallback(() => setPlaybackRate(1), [setPlaybackRate]);

  // Тональность: auto — производная от скорости (натуральный rodio-питч, ядро уже
  // так делает); manual — независимый сдвиг, требует DSP в ядре (пока стейт-only).
  const setPitchSemitones = useCallback((value: number) => setPitch(clampPitch(value)), []);
  const resetPitchSemitones = useCallback(() => setPitch(0), []);
  const setPitchMode = useCallback((mode: PitchMode) => setPitchModeState(mode), []);

  // Питч → ядро: manual — абсолютные полутоны (реальный DSP-сдвиг), auto — null (без
  // сдвига, питч следует за скоростью). Скорость ядро подхватывает живо, переслать не надо.
  useEffect(() => {
    void sc.player.setPitch(pitchMode === 'manual' ? pitchSemitones : null);
  }, [sc, pitchMode, pitchSemitones]);

  // Персист тюнинга (скорость/режим/питч) в config-стор: восстановление на старте
  // (ядро-процесс уже держит значения между релоадами; конфиг — для холодного старта
  // и консистентного UI). Сейв только после загрузки — иначе старт затрёт дефолтом.
  const tuningLoaded = useRef(false);
  useEffect(() => {
    let alive = true;
    void sc.config
      .get<{ rate: number; pitchMode: PitchMode; pitchSemitones: number }>('tuning')
      .then((t) => {
        if (alive && t) {
          if (Number.isFinite(t.rate)) setRate(clampRate(t.rate));
          if (t.pitchMode === 'manual' || t.pitchMode === 'auto') setPitchModeState(t.pitchMode);
          if (Number.isFinite(t.pitchSemitones)) setPitch(clampPitch(t.pitchSemitones));
        }
        tuningLoaded.current = true;
      })
      .catch(() => {
        tuningLoaded.current = true;
      });
    return () => {
      alive = false;
    };
  }, [sc]);
  useEffect(() => {
    if (!tuningLoaded.current) return;
    void sc.config.set('tuning', { rate: playbackRate, pitchMode, pitchSemitones });
  }, [sc, playbackRate, pitchMode, pitchSemitones]);

  const pushAb = useCallback(
    (ab: AbLoop | null) => {
      setAbLoop(ab);
      void sc.player.setAbLoop(ab?.a ?? null, ab?.b ?? null);
    },
    [sc],
  );

  const cycleAbPoint = useCallback(
    (pos: number) => {
      const at = Math.max(0, pos);
      setAbLoop((ab) => {
        let nextAb: AbLoop | null;
        if (!ab) nextAb = { a: at, b: null };
        else if (ab.b == null) {
          if (at > ab.a + AB_MIN_GAP) nextAb = { a: ab.a, b: at };
          else if (at < ab.a - AB_MIN_GAP) nextAb = { a: at, b: ab.a };
          else nextAb = null;
        } else nextAb = null;
        void sc.player.setAbLoop(nextAb?.a ?? null, nextAb?.b ?? null);
        return nextAb;
      });
    },
    [sc],
  );

  const nudgeAbBound = useCallback(
    (which: 'a' | 'b', value: number) => {
      setAbLoop((ab) => {
        if (!ab) return ab;
        const { a, b } = ab;
        let nextAb: AbLoop = ab;
        if (which === 'a') {
          const na = Math.max(0, value);
          if (b != null && na > b - AB_MIN_GAP) return ab;
          nextAb = { a: na, b };
        } else {
          const nb = Math.max(0, value);
          if (nb < a + AB_MIN_GAP) return ab;
          nextAb = { a, b: nb };
        }
        void sc.player.setAbLoop(nextAb.a, nextAb.b ?? null);
        return nextAb;
      });
    },
    [sc],
  );

  const clearAbLoop = useCallback(() => pushAb(null), [pushAb]);

  const extendQueue = useCallback((tracks: Track[]) => {
    setQueue((q) => {
      const seen = new Set(q.map((t) => t.id));
      const add = tracks.filter((t) => !seen.has(t.id));
      if (add.length === 0) return q;
      const merged = [...q, ...add];
      orderRef.current = merged;
      return merged;
    });
  }, []);

  const playNext = useCallback(
    (track: Track) => {
      setQueue((q) => {
        const filtered = q.filter((t) => t.id !== track.id);
        const at = Math.min(index + 1, filtered.length);
        const merged = [...filtered.slice(0, at), track, ...filtered.slice(at)];
        orderRef.current = merged;
        return merged;
      });
    },
    [index],
  );

  const reorderQueue = useCallback((from: number, to: number) => {
    const q = queueRef.current;
    if (from < 0 || from >= q.length || to < 0 || to >= q.length || from === to) return;
    const curId = q[indexRef.current]?.id;
    const nextQ = [...q];
    const [moved] = nextQ.splice(from, 1);
    nextQ.splice(to, 0, moved);
    orderRef.current = nextQ;
    setQueue(nextQ);
    const at = curId ? nextQ.findIndex((t) => t.id === curId) : indexRef.current;
    if (at >= 0) setIndex(at);
  }, []);

  const removeFromQueue = useCallback((id: string) => {
    const q = queueRef.current;
    const at = q.findIndex((t) => t.id === id);
    if (at < 0) return;
    const curId = q[indexRef.current]?.id;
    const nextQ = q.filter((t) => t.id !== id);
    orderRef.current = nextQ;
    setQueue(nextQ);
    const newIdx = curId === id ? Math.min(at, Math.max(0, nextQ.length - 1)) : nextQ.findIndex((t) => t.id === curId);
    setIndex(newIdx < 0 ? 0 : newIdx);
  }, []);

  // Анлайк убирает трек из «далее» (текущий играющий не трогаем).
  useEffect(
    () =>
      onUnlike((id) => {
        if (queueRef.current[indexRef.current]?.id === id) return;
        if (queueRef.current.some((t) => t.id === id)) removeFromQueue(id);
      }),
    [removeFromQueue],
  );

  // Восстановление последней сессии при открытии: очередь+индекс из config-стора.
  // Ядро — отдельный процесс, переживает перезагрузку страницы: если оно ЕЩЁ играет
  // тот трек — синкаем UI на playing + живую позицию ядра (иначе показывали бы паузу
  // при реально идущем звуке). Иначе — пауза, трек грузится на первый play.
  // Не перетираем, если юзер уже что-то запустил.
  useEffect(() => {
    let alive = true;
    void Promise.all([
      sc.config.get<{ queue: Track[]; index: number }>('playback'),
      sc.config.get<number>('playback-pos'),
      sc.player.isPlaying().catch(() => false),
      sc.player.position().catch(() => 0),
    ])
      .then(([saved, savedPos, corePlaying, corePos]) => {
        if (!alive || queueRef.current.length > 0) return;
        if (!saved || !Array.isArray(saved.queue) || saved.queue.length === 0) return;
        const idx = Math.min(Math.max(0, saved.index ?? 0), saved.queue.length - 1);
        orderRef.current = saved.queue;
        setQueue(saved.queue);
        setIndex(idx);
        const t = saved.queue[idx];
        if (t) setDuration(t.duration_ms);
        // Ядро играет и позиция в пределах трека → это тот же трек, синкаем как playing.
        const dur = t ? t.duration_ms / 1000 : 0;
        if (corePlaying && Number.isFinite(corePos) && (dur === 0 || corePos <= dur + 2)) {
          loadedRef.current = true;
          setPlaying(true);
          setPosition(corePos);
        } else {
          loadedRef.current = false;
          setPlaying(false);
          const p = Number.isFinite(savedPos as number) ? (savedPos as number) : 0;
          restoredPosRef.current = p;
          setPosition(p);
        }
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [sc]);

  // Персист: очередь+индекс (дебаунс на смену трека) и позиция (интервал + на паузу).
  useEffect(() => {
    const t = setTimeout(() => {
      if (queueRef.current.length > 0) void sc.config.set('playback', { queue: queueRef.current, index: indexRef.current });
    }, 800);
    return () => clearTimeout(t);
  }, [queue, index, sc]);

  useEffect(() => {
    if (!playing) {
      if (loadedRef.current) void sc.config.set('playback-pos', getPositionSecs());
      return;
    }
    const iv = setInterval(() => void sc.config.set('playback-pos', getPositionSecs()), 5000);
    return () => clearInterval(iv);
  }, [playing, sc]);

  const value = useMemo<PlayerState>(
    () => ({
      queue, index, currentTrack, playing, volume, volumeBeforeMute, shuffle, repeat,
      abLoop, playbackRate, pitchSemitones, pitchMode,
      playQueue, toggle, togglePlayPause, next, prev, toggleShuffle, cycleRepeat,
      seek, setVolume, toggleMute, setPlaybackRate, resetPlaybackRate,
      setPitchSemitones, resetPitchSemitones, setPitchMode,
      cycleAbPoint, nudgeAbBound, clearAbLoop,
      extendQueue, playNext, reorderQueue, removeFromQueue,
    }),
    [queue, index, currentTrack, playing, volume, volumeBeforeMute, shuffle, repeat, abLoop, playbackRate, pitchSemitones, pitchMode, playQueue, toggle, togglePlayPause, next, prev, toggleShuffle, cycleRepeat, seek, setVolume, toggleMute, setPlaybackRate, resetPlaybackRate, setPitchSemitones, resetPitchSemitones, setPitchMode, cycleAbPoint, nudgeAbBound, clearAbLoop, extendQueue, playNext, reorderQueue, removeFromQueue],
  );

  return <PlayerCtx.Provider value={value}>{children}</PlayerCtx.Provider>;
}

function shuffleKeepingFirst(list: Track[], keepIndex: number): Track[] {
  const first = list[keepIndex];
  const rest = list.filter((_, i) => i !== keepIndex);
  for (let i = rest.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [rest[i], rest[j]] = [rest[j], rest[i]];
  }
  return first ? [first, ...rest] : rest;
}
