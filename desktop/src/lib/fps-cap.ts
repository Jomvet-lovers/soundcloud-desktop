//! Кап частоты кадров.
//!
//! В windowed-вебвью (и wry/webkitgtk, и CEF/chromium) анимации идут с частотой
//! развёртки монитора — на 120/144 Гц это лишние CPU/GPU на ту же картинку.
//! Chromium-флага «cap до N fps» для windowed нет (`windowless_frame_rate` —
//! только off-screen). Поэтому троттлим `requestAnimationFrame`: пропущенные
//! кадры переназначаем (чтобы rAF-циклы не рвались), `cancelAnimationFrame`
//! сохраняем рабочим. На дисплеях ≤ целевого FPS — фактически no-op.

const DEFAULT_FPS = 60;

let installed = false;

export function installFpsCap(targetFps: number = DEFAULT_FPS): void {
  if (installed || targetFps <= 0 || typeof window === 'undefined') return;
  installed = true;

  const minDelta = 1000 / targetFps;
  const rafNative = window.requestAnimationFrame.bind(window);
  const cafNative = window.cancelAnimationFrame.bind(window);

  // Наш handle → текущий нативный id (живёт через переназначения пропущенных
  // кадров, чтобы cancelAnimationFrame отменял именно ожидающий кадр).
  const pending = new Map<number, number>();
  let nextHandle = 1;
  let last = 0;

  window.requestAnimationFrame = (cb: FrameRequestCallback): number => {
    const handle = nextHandle++;
    const tick = (now: number) => {
      if (now - last >= minDelta - 1) {
        last = now;
        pending.delete(handle);
        cb(now);
      } else {
        pending.set(handle, rafNative(tick));
      }
    };
    pending.set(handle, rafNative(tick));
    return handle;
  };

  window.cancelAnimationFrame = (handle: number): void => {
    const nativeId = pending.get(handle);
    if (nativeId !== undefined) {
      pending.delete(handle);
      cafNative(nativeId);
    } else {
      // id не из нашего пула (например, выдан до установки капа) — в нативный.
      cafNative(handle);
    }
  };
}
