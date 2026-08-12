# Desktop/depens — десктоп-онли крейты (Rust)

То, что нужно/возможно только на десктопе (mac/win/linux) и чего нет на мобилках.
Кросс-платформенное (в т.ч. ios/android) живёт в `Core/shared`, не здесь.

Планируемые крейты (перенос с легаси `Desktop/desktop/src-tauri`):
- **tray** — иконка и меню в системном трее + поповер-миниплеер.
- **media-controls** — MPRIS (Linux) / SMTC (Windows) / NowPlaying (macOS) через `souvlaki`.
- **discord** — Rich Presence (IPC).
- **single-instance**, оконные хелперы, GPU-воркэраунды (Linux NVIDIA/Wayland), rlimit.
- **bridge** — собственный мост этих фич в Flutter (отдельно от `sc-bridge`,
  потому что Core не зависит от Desktop).

Подключение к ядру:
- Core тянем git-зависимостью (`sc-core` и пр.), локально — `[patch]` на `../../Core`.
  Снаппеты — в `../../Core/CLAUDE.md`.
- Эти крейты реализуют **порты платформы**, объявленные в `sc-core` (медиа-контролы,
  трей и т.п.) — ядро дёргает их через трейты, не зная конкретики.

Правила кода — `../../Core/CLAUDE.md` (человекочитаемо, SOLID/DI, без unwrap в проде,
cargo add, без глушения варнингов).
