# Rust Refactor Audit

Current snapshot: 2026-03-22

## Native hot command surface

`desktop/src/lib/audio.ts` currently drives the hottest native calls:

- `audio_play`, `audio_pause`, `audio_stop`, `audio_seek`
- `audio_set_volume`
- `audio_set_eq`
- `audio_set_normalization`
- `audio_set_metadata`
- `audio_set_playback_state`
- `audio_set_media_position`
- `audio_list_devices`
- `audio_switch_device`
- `audio_load_file`, `audio_load_url`

Classification:

- Hot path: `audio_play`, `audio_pause`, `audio_seek`, `audio_set_volume`, `audio_set_media_position`
- Background polling / recurring checks: `audio_list_devices`
- Heavy user-triggered / track lifecycle: `audio_load_file`, `audio_load_url`, `audio_switch_device`
- Infrequent metadata sync: `audio_set_metadata`, `audio_set_playback_state`, `audio_set_eq`, `audio_set_normalization`

## Frontend-owned timers and loops to migrate

Current JS ownership in `desktop/src/lib/audio.ts`:

- 1s stall detector interval that reloads tracks when no `audio:tick` arrives
- 10s default-output polling interval built on `audio_list_devices`
- visibility resume logic that infers device death from stale ticks
- track reload sequencing and device-follow state

Current JS ownership in `desktop/src/lib/diagnostics.ts`:

- 1s UI watchdog interval for event-loop lag
- slow `invoke(...)` and async task timing wrappers

Migration targets for Rust ownership:

- Audio device default-follow and failover should move behind a native scheduler and emit `audio:default_device_changed` or `audio:device-reconnected`.
- Stall detection should move next to the tick/device state machine so the UI stops inferring native failure from missing events.
- Playback-adjacent timers for lyrics and floating comments should use the same native pattern: one session payload in, sparse events out.

## Completed in this slice

- `desktop/src-tauri/src/audio_player.rs` was replaced with a folder-based `desktop/src-tauri/src/audio/` module tree.
- The Tauri command names are preserved, but the code now separates command entrypoints from engine, device, decode, EQ, media controls, tick emission, state, and shared types.
- Default output follow and stall recovery for audio were moved out of `desktop/src/lib/audio.ts` and into native background logic.
- Large track surfaces now use a shared virtual list in React on:
  - playlist page for non-owner track lists
  - search track results
  - user tracks / popular / likes

## Immediate next slice

- Move synced lyrics and floating-comments scheduling to the same native session/event pattern used for audio device timing.
- Extend virtualization to more long surfaces, especially library track views and any owner-playlist mode that can preserve drag/reorder behavior without regressing UX.
- Continue collapsing the remaining flat `src-tauri/src` modules into domain folders (`app`, `network`, `discord`, `import`, `shared`).
