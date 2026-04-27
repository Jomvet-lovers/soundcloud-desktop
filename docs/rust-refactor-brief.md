# Rust Refactor Brief

## Goal

Move the critical long-running and OS-facing parts of the desktop app out of the React layer into Rust, while keeping React as the UI/rendering shell.

At the same time:

- split `desktop/src-tauri/src/audio_player.rs` into focused modules
- stop growing `desktop/src-tauri/src` as a flat folder
- reduce UI freezes caused by heavy timers, IPC pressure, device enumeration, and large stateful logic in JS
- improve rendering scalability for very large playlists and feeds

## Constraints

- React stays as the app UI layer
- existing user-facing behavior must be preserved unless explicitly improved
- no big-bang rewrite; work should be incremental and shippable
- all new native subsystems must communicate through narrow Tauri commands/events
- performance and stability are higher priority than architectural purity
- avoid pseudo-solutions that sound fast but require replacing the whole UI stack

## Current pain points

### Frontend

- too much long-running stateful logic in JS/TS
- periodic `invoke(...)` calls from the UI layer
- timer-driven subsystems in the frontend (`lyrics`, `floating comments`, watchdog/polling logic)
- large list rendering and dedupe/merge work on the React side
- long DOM trees for playlists/feeds can overload layout, paint, and compositing
- debugging freezes is hard because the critical path is spread across many frontend files

### Rust side

- `audio_player.rs` is too large and mixes multiple responsibilities
- `src-tauri/src` is flat and does not scale
- audio, device management, playback state, media controls, and normalization are tightly packed together

## Target architecture

### React responsibilities

- routes, screens, layout, controls, dialogs
- rendering track/user/playlist data
- dispatching commands to native services
- subscribing to native events and painting UI state

### Rust responsibilities

- audio engine and playback state machine
- output-device management and auto-switch/failover logic
- timed schedulers for playback-adjacent features
- filesystem-heavy work
- heavy aggregation/transforms that are repeatedly recalculated on the frontend
- diagnostics/logging pipeline

## Rendering reality check

### What is realistic

- keep React as the UI layer and optimize the existing WebView path
- reduce DOM size with virtualization
- prevent offscreen work with CSS/runtime techniques where supported
- move timing/stateful heavy work to Rust while React remains the renderer

### What is not realistic as a short-term optimization

- "render the current React interface directly through GPU via Skia and skip the browser engine"

With the current Tauri + WebView + React architecture, the UI is still rendered by the embedded browser engine.
Switching to a Skia-driven renderer or similar would mean replacing the UI runtime, not just optimizing it.

That is effectively a platform rewrite and is out of scope for the current refactor unless the team explicitly decides to leave the WebView model behind.

### Acceptable GPU-related work

- keep and improve platform-specific GPU workarounds in native startup
- investigate an experimental hardware-acceleration toggle only if it can be applied safely before WebView initialization and documented as restart-required
- optimize paint/layout/compositing pressure inside the current WebView architecture

## Priority phases

## Phase 1: Stabilize diagnostics and ownership boundaries

- keep the current UI watchdog and file logging
- standardize a single diagnostics path for native/frontend warnings and slow operations
- audit all `invoke(...)` usage and classify commands by:
  - hot path
  - user-triggered
  - background polling
- identify frontend loops/timers that should no longer own state

Deliverable:

- clear map of hot commands and recurring lag sources

## Phase 2: Refactor audio subsystem into modules

Replace the flat `audio_player.rs` with a folder-based layout.

Suggested structure:

```text
desktop/src-tauri/src/
  audio/
    mod.rs
    commands.rs
    state.rs
    engine.rs
    device.rs
    decode.rs
    eq.rs
    normalization.rs
    media_controls.rs
    tick.rs
    types.rs
```

Responsibilities:

- `commands.rs`: Tauri command entrypoints only
- `state.rs`: `AudioState`, shared flags, channels, ownership
- `engine.rs`: player creation/reload/seek/stop/load orchestration
- `device.rs`: output device discovery, switching, reconnect, default-follow logic
- `decode.rs`: decoder setup and source creation
- `eq.rs`: EQ source/filter code
- `normalization.rs`: gain analysis and normalization helpers
- `media_controls.rs`: SMTC/MPRIS integration
- `tick.rs`: tick emitter / ended detection / event emission
- `types.rs`: shared structs and DTOs

Deliverable:

- same behavior, but `audio_player.rs` reduced to a thin module entry or removed entirely

## Phase 2.5: Organize native code by domain, not files

Target direction:

```text
desktop/src-tauri/src/
  app/
    mod.rs
    diagnostics.rs
    tray.rs
  audio/
    ...
  discord/
    mod.rs
    rpc.rs
    commands.rs
  network/
    mod.rs
    proxy.rs
    proxy_server.rs
    static_server.rs
    server.rs
  import/
    mod.rs
    ym.rs
  shared/
    mod.rs
    constants.rs
```

The exact shape can change, but the flat top-level file pile should go away.

Deliverable:

- `src-tauri/src` reads as domains/subsystems, not miscellaneous large files

## Phase 3: Move timed playback-adjacent UI logic to Rust

Candidates:

- synced lyrics timing
- floating comments timing/scheduling
- output-device polling/follow-default logic

Pattern:

- frontend sends session/input data once
- Rust owns the timer/state machine
- Rust emits small events like:
  - `lyrics:active_line`
  - `comments:show`
  - `audio:default_device_changed`

Frontend should stop doing high-frequency time comparisons for these features.

Deliverable:

- React becomes a view layer for playback timing features instead of the timing owner

## Phase 4: Reduce frontend heavy data work

Audit large list screens:

- playlist page
- user page
- search page
- library/home feeds

Move repeated heavy transforms out of render paths:

- repeated dedupe/merge logic
- track annotation/normalization glue
- multi-source merging that can be done once upstream

Preferred approach:

- first move heavy shaping to backend/native service if it is repeatedly reused
- otherwise memoize/workerize on frontend only if native ownership adds no value

Deliverable:

- less render-time CPU work and fewer long synchronous JS frames

## Phase 5: Rendering scalability for large playlists and feeds

### 5.1 Virtualized lists

Implement virtualization for long track/user/playlist lists so the app only renders the visible slice plus overscan.

Targets:

- playlist track lists
- library liked tracks/history
- user tracks/likes/followings
- search results

Expectation:

- render ~10-30 visible rows instead of hundreds/thousands
- large playlists should no longer create massive DOM trees

Preferred implementation path:

- keep this in React, using a virtual list/windowing approach
- only move list measurement/math to native if profiling proves the JS side is still a bottleneck

### 5.2 Offscreen rendering suppression

Where virtualization is not practical or not yet implemented, use browser-level containment aggressively:

- `content-visibility: auto`
- `contain: layout paint style`
- deferred image loading / decoding
- avoid expensive shadows/filters on huge repeated lists

This is not a substitute for virtualization, but it is a useful secondary optimization.

### 5.3 Track row cost audit

Audit the per-row cost of track items:

- image decode/resize
- hover effects
- gradients/shadows/blur
- button subtrees
- unnecessary rerenders from global store subscriptions

Deliverable:

- 500-1000 item playlists scroll smoothly with virtualization
- non-virtualized surfaces do less offscreen work

## Experimental track: hardware acceleration toggle

This can be explored, but only under strict limits.

Facts:

- hardware acceleration behavior is platform-specific
- some toggles must be applied before WebView startup
- on some systems disabling acceleration helps stability; on others it makes rendering worse
- this is likely restart-required

Therefore:

- do not promise a universal in-app toggle without platform validation
- if implemented, mark it experimental and restart-required
- Linux startup env-based workarounds are the first acceptable place to prototype this
- Windows/WebView2 behavior must be verified separately before productizing a setting

## Acceptance criteria

- React no longer owns critical playback-adjacent timers
- `audio_player.rs` is modularized and no longer a monolith
- `src-tauri/src` is organized by domain, not a flat list of large files
- diagnostics make it obvious whether a freeze came from:
  - event-loop lag
  - slow Tauri command
  - slow HTTP
  - audio device / native subsystem issues
- large playlist/feed screens remain responsive due to virtualization and reduced offscreen work
- no regression in:
  - playback
  - seek
  - output switching
  - RPC
  - import/cache flows

## Non-goals

- removing React
- rewriting the whole app in Rust
- replacing the WebView renderer with Skia/another custom GPU UI runtime as part of this refactor
- moving every tiny UI behavior into native code
- large visual redesign during refactor

## Implementation rule of thumb

If a feature:

- depends on OS/audio/filesystem
- runs on timers for the lifetime of playback
- performs repeated heavy computation
- can wedge the UI if it stalls

then it should be considered a Rust-side candidate.

If a feature is primarily:

- layout
- rendering
- local interaction state
- page composition

then it should stay in React.
