use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::backend::audio::engine;
use crate::backend::audio::state::AudioState;
use crate::backend::audio::timing;
use crate::backend::audio::types::{
    AudioThreadCmd, STALL_COOLDOWN_MS, STALL_THRESHOLD_MS, TICK_INTERVAL_MS,
};
use crate::backend::events::EventBus;

// TODO(egui-port): Tauri の `app::diagnostics::log_native(handle, …)` 相当は未移植
// (`backend/app` は Phase 1 の他タスク)。現状は `eprintln!` に縮退し、Phase 5 で
// ファイルログへ再接続する。

/// Step the hover-preview volume one tick toward its target, dropping the player
/// once a fade-out reaches zero. Independent of the main player.
fn process_preview_fade(state: &AudioState) {
    let mut preview = state.preview.lock().unwrap();
    if preview.player.is_none() || (preview.volume - preview.target).abs() <= f32::EPSILON {
        return;
    }
    let next = if preview.volume < preview.target {
        (preview.volume + preview.step).min(preview.target)
    } else {
        (preview.volume - preview.step).max(preview.target)
    };
    preview.volume = next;
    if let Some(ref player) = preview.player {
        player.set_volume(next);
    }
    if preview.stop_at_zero && next <= 0.0
        && let Some(old) = preview.player.take() {
            old.stop();
        }
}

// TODO(egui-port): 元は `start_tick_emitter(app: &AppHandle)` で state を
// `handle.state::<AudioState>()` から取得。Tauri の managed-state が無いため
// `bus: EventBus` + `state: Arc<AudioState>` を Phase 2 の `boot()` から明示的に渡す。
pub fn start_tick_emitter(bus: EventBus, state: std::sync::Arc<AudioState>) {
    std::thread::Builder::new()
        .name("audio-tick".into())
        .spawn(move || {
            let mut last_pos_ms = 0u64;
            let mut last_progress_at = std::time::Instant::now();
            let mut stall_cooldown_until = std::time::Instant::now();

            loop {
                std::thread::sleep(Duration::from_millis(TICK_INTERVAL_MS));

                if state.device_reconnected.swap(false, Ordering::Acquire) {
                    let _ = engine::reload_current_track(&state);
                    eprintln!("[Audio] Device reconnected and reloaded");
                    bus.emit("audio:device-reconnected", &());
                }

                // Advance the hover-preview volume ramp. Done before the has_track
                // guard so previews fade in/out even when no main track is loaded.
                process_preview_fade(&state);

                if !state.has_track.load(Ordering::Relaxed) {
                    last_pos_ms = 0;
                    last_progress_at = std::time::Instant::now();
                    continue;
                }

                let player_guard = state.player.lock().unwrap();
                if let Some(ref player) = *player_guard {
                    if player.empty() {
                        let suppress_ended = super::engine::now_ms()
                            < state.suppress_ended_until_ms.load(Ordering::Relaxed);
                        if !state.device_error.load(Ordering::Relaxed)
                            && !suppress_ended
                            && !state.ended_notified.swap(true, Ordering::Relaxed)
                        {
                            bus.emit("audio:ended", &());
                        }
                    } else {
                        // rodio's get_pos() is output (wall-clock) time; the rest of the
                        // app works in source seconds, so integrate (exact across speed
                        // changes — see engine::source_pos).
                        let rate = engine::current_rate(&state);
                        let raw = player.get_pos().as_secs_f64();
                        let pos = engine::source_pos(&state, player);

                        // A-B loop: snap back to A as soon as we cross B (source secs).
                        // Route through engine::seek_to (in-place try_seek with a
                        // recreate fallback), NOT a bare try_seek: on decoders that
                        // can't seek in place a bare try_seek silently no-ops, leaving
                        // the segment playing straight through while the bar froze at A.
                        let ab = *state.ab_loop.lock().unwrap();
                        if let Some((a, b)) = ab
                            && pos >= b {
                                drop(player_guard);
                                engine::seek_to(&state, a).ok();
                                bus.emit("audio:tick", &a);
                                last_pos_ms = ((a / rate).max(0.0) * 1000.0) as u64;
                                last_progress_at = std::time::Instant::now();
                                continue;
                            }

                        bus.emit("audio:tick", &pos);
                        timing::process_lyrics_timeline(&bus, &state, pos);
                        timing::process_comments_timeline(&bus, &state, pos);

                        let playing = !player.is_paused();
                        let pos_ms = (raw * 1000.0) as u64;
                        let now = std::time::Instant::now();

                        if !playing {
                            last_pos_ms = pos_ms;
                            last_progress_at = now;
                            continue;
                        }

                        if pos_ms > last_pos_ms {
                            last_pos_ms = pos_ms;
                            last_progress_at = now;
                            continue;
                        }

                        // Backward seek detected — reset stall tracking
                        if pos_ms < last_pos_ms.saturating_sub(500) {
                            last_pos_ms = pos_ms;
                            last_progress_at = now;
                            continue;
                        }

                        // Don't mistake a settling device switch/reconnect for a stall:
                        // the freshly opened output may not be pulling samples yet.
                        if super::engine::now_ms()
                            < state.suppress_stall_until_ms.load(Ordering::Relaxed)
                        {
                            last_progress_at = now;
                            continue;
                        }

                        if now < stall_cooldown_until {
                            continue;
                        }

                        if now.duration_since(last_progress_at).as_millis() as u64
                            > STALL_THRESHOLD_MS
                        {
                            drop(player_guard);
                            eprintln!("[Audio] Stall detected, reconnecting audio device");
                            // Reconnect device — stall often means the audio stream
                            // died silently (macOS sleep/wake, headphone unplug).
                            // Just reloading the track on a dead mixer won't help.
                            state.audio_tx.send(AudioThreadCmd::Reconnect).ok();
                            stall_cooldown_until = std::time::Instant::now()
                                + Duration::from_millis(STALL_COOLDOWN_MS);
                            last_progress_at = std::time::Instant::now();
                        }
                    }
                }
            }
        })
        .expect("failed to spawn tick thread");
}
