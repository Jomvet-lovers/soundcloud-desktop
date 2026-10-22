use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use tauri::State;
use tokio::task;

use crate::audio::decode::{
    create_player_from_bytes, create_player_from_stream, resolve_normalization_gain,
    resolve_prefix_normalization_gain,
};
use crate::audio::state::AudioState;
use crate::audio::stream_buffer::StreamBuffer;
use crate::audio::types::{AudioLoadResult, MediaCmd, EQ_BANDS, STALL_SUPPRESS_MS, TICK_INTERVAL_MS};

const ENDED_SUPPRESS_MS: u64 = 1200;
/// Cap on the prefix decoded for the loudness gain when playback starts on a
/// partial download. The analysis only looks at the first 30 s of samples, which
/// the prebuffer covers at SoundCloud's bitrates; a short estimate is replaced by
/// the exact gain when the finished bytes are adopted.
const PREFIX_ANALYSIS_BYTES: usize = 1_500_000;

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn suppress_ended_temporarily(state: &AudioState) {
    state
        .suppress_ended_until_ms
        .store(now_ms() + ENDED_SUPPRESS_MS, Ordering::Relaxed);
}

/// Mute stall-detection (tick.rs) for a short window. A device switch briefly
/// freezes the old player and the freshly opened output may lag before it starts
/// pulling samples; without this the tick thread reads the gap as a dead stream and
/// fires a redundant reconnect mid-switch.
pub fn suppress_stall_temporarily(state: &AudioState) {
    state
        .suppress_stall_until_ms
        .store(now_ms() + STALL_SUPPRESS_MS, Ordering::Relaxed);
}

fn volume_to_rodio(v: f64) -> f32 {
    (v / 100.0).clamp(0.0, 2.0) as f32
}

fn stop_player_only(state: &AudioState) {
    suppress_ended_temporarily(state);
    let mut player = state.player.lock().unwrap();
    if let Some(old) = player.take() {
        old.stop();
    }
}

fn stop_current_player(state: &AudioState) {
    // A streaming session ends with its player: cancel the mirror first so a
    // decoder parked in a blocking read returns EOF and the player can be
    // dropped, then drop the player.
    if let Some(stream) = state.stream.lock().unwrap().take() {
        stream.cancel();
    }
    state.streaming.store(false, Ordering::Relaxed);
    stop_player_only(state);
}

/// Install a streaming-first-play session: replaces (and cancels) whatever was
/// playing, and makes `stream` the token every later step of that session must
/// still match (track switch / stop clears it).
pub fn begin_stream(state: &AudioState, stream: &Arc<StreamBuffer>) {
    stop_current_player(state);
    *state.stream.lock().unwrap() = Some(stream.clone());
    state.streaming.store(true, Ordering::Relaxed);
}

/// Whether `stream` is still the session the current track owns.
pub fn is_active_stream(state: &AudioState, stream: &Arc<StreamBuffer>) -> bool {
    state
        .stream
        .lock()
        .unwrap()
        .as_ref()
        .map(|active| Arc::ptr_eq(active, stream))
        .unwrap_or(false)
}

/// Current playback speed as f64, floored away from zero so it's safe to divide by.
/// rodio's `get_pos()`/`try_seek()` operate in output (wall-clock) time = source/rate;
/// this lets callers convert to/from the source timeline the rest of the app uses.
pub fn current_rate(state: &AudioState) -> f64 {
    (*state.playback_rate.lock().unwrap() as f64).max(0.01)
}

/// Current position in **source seconds**, integrated from rodio's wall-clock get_pos().
/// Exact across mid-track speed changes (each constant-rate segment is closed into the
/// anchor by set_playback_rate). Does NOT lock `state.player` — pass the held player.
pub fn source_pos(state: &AudioState, player: &rodio::Player) -> f64 {
    let rate = current_rate(state);
    let (src_anchor, out_anchor) = *state.pos_anchor.lock().unwrap();
    (src_anchor + (player.get_pos().as_secs_f64() - out_anchor) * rate).max(0.0)
}

/// Re-base the integrator so subsequent get_pos() readings map to source `source`.
/// `output` is the get_pos() value that corresponds to that source position right now.
fn set_pos_anchor(state: &AudioState, source: f64, output: f64) {
    *state.pos_anchor.lock().unwrap() = (source.max(0.0), output.max(0.0));
}

fn commit_loaded_track(
    state: &AudioState,
    bytes: Vec<u8>,
    new_player: rodio::Player,
    normalization_gain: f32,
) {
    *state.player.lock().unwrap() = Some(Arc::new(new_player));
    *state.source_bytes.lock().unwrap() = Some(bytes);
    *state.normalization_gain.lock().unwrap() = normalization_gain;
    // Fresh track starts at source 0 / output 0.
    set_pos_anchor(state, 0.0, 0.0);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);
    state.device_error.store(false, Ordering::Relaxed);
}

/// Install a player that reads its audio from the live download. Unlike
/// `commit_loaded_track` there are no complete bytes yet: seeks fall back to the
/// buffer (see `rebuild_from_stream`) and `finalize_stream` adopts the finished
/// file later.
fn commit_stream_player(state: &AudioState, new_player: rodio::Player, normalization_gain: f32) {
    *state.player.lock().unwrap() = Some(Arc::new(new_player));
    *state.source_bytes.lock().unwrap() = None;
    *state.normalization_gain.lock().unwrap() = normalization_gain;
    set_pos_anchor(state, 0.0, 0.0);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);
    state.device_error.store(false, Ordering::Relaxed);
}

// Все 9 параметров — это один build шаг плеера: mixer/volume/normalization-кеш
// + eq + analyser идут в одну `spawn_blocking`-обертку. Заводить отдельную
// структуру `BuildPlayerArgs` ради одной точки вызова — лишний слой.
#[allow(clippy::too_many_arguments)]
async fn build_player_from_bytes(
    bytes: Vec<u8>,
    mixer: rodio::mixer::Mixer,
    volume: f32,
    normalization_enabled: bool,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
    start_paused: bool,
    output_sample_rate: u32,
    speed: std::sync::Arc<std::sync::atomic::AtomicU32>,
    eq_params: std::sync::Arc<std::sync::RwLock<crate::audio::types::EqParams>>,
    analyser_buffer: std::sync::Arc<crate::audio::analyser::AnalyserBuffer>,
) -> Result<(Vec<u8>, rodio::Player, Option<f64>, f32), String> {
    task::spawn_blocking(move || {
        let normalization_gain = if normalization_enabled {
            resolve_normalization_gain(
                &bytes,
                normalization_cache_dir.as_deref(),
                normalization_cache_key.as_deref(),
            )?
        } else {
            1.0
        };
        let (player, duration_secs) = create_player_from_bytes(
            &bytes,
            &mixer,
            volume,
            normalization_gain,
            start_paused,
            output_sample_rate,
            speed,
            eq_params,
            analyser_buffer,
        )?;
        Ok((bytes, player, duration_secs, normalization_gain))
    })
    .await
    .map_err(|e| format!("audio decode task failed: {e}"))?
}

pub fn reload_current_track(state: &AudioState) -> Result<(), String> {
    suppress_ended_temporarily(state);
    suppress_stall_temporarily(state);
    let bytes = state.source_bytes.lock().unwrap().clone();
    let Some(bytes) = bytes else {
        // Still streaming (download unfinished): rebuild from the live buffer so
        // a device switch does not leave the track silent.
        let stream = state.stream.lock().unwrap().clone();
        let Some(stream) = stream else {
            return Ok(());
        };
        let (source_position, was_paused) = {
            let player = state.player.lock().unwrap();
            let Some(player) = player.as_ref() else {
                return Ok(());
            };
            (source_pos(state, player), player.is_paused())
        };
        return rebuild_from_stream(state, &stream, source_position, was_paused, None);
    };

    let rate = current_rate(state);
    let (source_position, was_paused) = {
        let player = state.player.lock().unwrap();
        let Some(player) = player.as_ref() else {
            return Ok(());
        };
        (source_pos(state, player), player.is_paused())
    };

    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let normalization_gain = *state.normalization_gain.lock().unwrap();
    let (new_player, _) = create_player_from_bytes(
        &bytes,
        &mixer,
        vol,
        if normalization_enabled {
            normalization_gain
        } else {
            1.0
        },
        was_paused,
        state.output_rate.load(Ordering::Relaxed),
        state.playback_rate_fp.clone(),
        state.eq_params.clone(),
        state.analyser_buffer.clone(),
    )?;
    // Apply speed BEFORE seeking so try_seek's argument is interpreted under the speed
    // factor: try_seek(source/rate) lands the decoder at the original source position.
    let output_target = source_position / rate;
    if source_position > 0.0 {
        new_player
            .try_seek(Duration::from_secs_f64(output_target))
            .ok();
    }

    let mut player = state.player.lock().unwrap();
    if let Some(old) = player.take() {
        old.stop();
    }
    *player = Some(Arc::new(new_player));
    set_pos_anchor(state, source_position, output_target);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);
    state.device_error.store(false, Ordering::Relaxed);

    Ok(())
}

pub async fn load_file(
    path: String,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
    start_paused: bool,
    state: State<'_, AudioState>,
) -> Result<AudioLoadResult, String> {
    load_file_inner(
        &state,
        path,
        normalization_cache_dir,
        normalization_cache_key,
        start_paused,
        None,
    )
    .await
}

/// `load_file` without a `State` handle, so the streaming command's background
/// task can load the finished file on the fallback path.
///
/// `expected_stream` binds the load to a streaming session: the player is only
/// installed while that session is still the current one. A cache-miss full-wait
/// load can overlap a newer track (the download finished, then the user clicked
/// another row); without the guard the stale load would stop the new track and
/// install itself over it.
pub async fn load_file_inner(
    state: &AudioState,
    path: String,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
    start_paused: bool,
    expected_stream: Option<&Arc<StreamBuffer>>,
) -> Result<AudioLoadResult, String> {
    if let Some(stream) = expected_stream
        && !is_active_stream(state, stream)
    {
        return Err("stream cancelled".into());
    }

    // The full-wait path has no stream session to bind to, so bind it to the
    // engine generation instead. `stop()` bumps it, and every new load stops
    // first: a load that finishes after the user picked another track must not
    // stop that track or install itself over it (see `load_url`, same idiom).
    let generation = state.load_gen.load(Ordering::Relaxed);
    let superseded = || state.load_gen.load(Ordering::Relaxed) != generation;
    let empty_result = AudioLoadResult {
        duration_secs: None,
    };

    let bytes = task::spawn_blocking({
        let path = path.clone();
        move || std::fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path, e))
    })
    .await
    .map_err(|e| format!("audio file read task failed: {e}"))??;

    match expected_stream {
        Some(stream) => {
            // Re-check after the read, then silence the old player without
            // clearing the session slot: the install below still has to pass
            // the same check under that lock.
            if !is_active_stream(state, stream) {
                return Err("stream cancelled".into());
            }
            stop_player_only(state);
        }
        None => {
            if superseded() {
                return Ok(empty_result);
            }
            stop_current_player(state);
        }
    }

    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let (bytes, new_player, duration_secs, normalization_gain) = build_player_from_bytes(
        bytes,
        mixer,
        vol,
        normalization_enabled,
        normalization_cache_dir,
        normalization_cache_key,
        start_paused,
        state.output_rate.load(Ordering::Relaxed),
        state.playback_rate_fp.clone(),
        state.eq_params.clone(),
        state.analyser_buffer.clone(),
    )
    .await?;

    match expected_stream {
        Some(stream) => {
            // Holding the session lock across check and install is what makes a
            // switch during the decode win: the switch path takes it first.
            let mut slot = state.stream.lock().unwrap();
            if !slot
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(active, stream))
            {
                new_player.stop();
                return Err("stream cancelled".into());
            }
            if let Some(old) = state.player.lock().unwrap().take() {
                old.stop();
            }
            commit_loaded_track(state, bytes, new_player, normalization_gain);
            *slot = None;
            state.streaming.store(false, Ordering::Relaxed);
        }
        None => {
            if superseded() {
                new_player.stop();
                return Ok(empty_result);
            }
            commit_loaded_track(state, bytes, new_player, normalization_gain);
        }
    }

    Ok(AudioLoadResult { duration_secs })
}

/// Start playback on a download that is still arriving.
///
/// `begin_stream` must have installed `stream` as the current session. The build
/// blocks until the decoder has its first frame (the caller already waited for a
/// prebuffer), then the player reads straight from the buffer and blocks when it
/// catches up with the network. Failures are the caller's cue to fall back to the
/// full-file path.
pub async fn load_stream(
    stream: Arc<StreamBuffer>,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
    start_paused: bool,
    state: &AudioState,
) -> Result<AudioLoadResult, String> {
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    // The analysis window is the first 30 s, so the buffered prefix gives the
    // same gain as the finished file. Skipped entirely when normalization is off
    // (the default) so it never delays the first sample.
    let normalization_gain = if normalization_enabled {
        let head = stream.head(PREFIX_ANALYSIS_BYTES);
        let cache_dir = normalization_cache_dir.clone();
        let cache_key = normalization_cache_key.clone();
        task::spawn_blocking(move || {
            resolve_prefix_normalization_gain(&head, cache_dir.as_deref(), cache_key.as_deref())
        })
        .await
        .unwrap_or(1.0)
    } else {
        1.0
    };

    let byte_len = stream.byte_len();
    let reader = stream.reader();
    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let output_rate = state.output_rate.load(Ordering::Relaxed);
    let speed = state.playback_rate_fp.clone();
    let eq_params = state.eq_params.clone();
    let analyser_buffer = state.analyser_buffer.clone();
    let (new_player, duration_secs) = task::spawn_blocking(move || {
        create_player_from_stream(
            reader,
            &mixer,
            vol,
            normalization_gain,
            start_paused,
            output_rate,
            byte_len,
            speed,
            eq_params,
            analyser_buffer,
        )
    })
    .await
    .map_err(|e| format!("audio stream decode task failed: {e}"))??;

    // A track switch or stop may have landed while the decoder was building.
    if !is_active_stream(state, &stream) {
        new_player.stop();
        return Err("stream cancelled".into());
    }
    commit_stream_player(state, new_player, normalization_gain);

    Ok(AudioLoadResult { duration_secs })
}

/// Adopt the finished download: the complete bytes become the authoritative
/// source (exact duration, seek fallback, cached loudness gain) and the player is
/// rebuilt in place at the current position. Returns false when the session was
/// already replaced.
///
/// Blocking (full decode + gain analysis): call it from `spawn_blocking`.
pub fn finalize_stream(
    state: &AudioState,
    stream: &Arc<StreamBuffer>,
    bytes: Vec<u8>,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
) -> bool {
    // Hold the session slot across the whole swap: a concurrent load/stop takes
    // the same lock before installing its own source, so nothing can slip in
    // between the identity check and the rebuild.
    let mut slot = state.stream.lock().unwrap();
    match slot.as_ref() {
        Some(active) if Arc::ptr_eq(active, stream) => {}
        _ => return false,
    }

    let normalization_gain = if state.normalization_enabled.load(Ordering::Relaxed) {
        resolve_normalization_gain(
            &bytes,
            normalization_cache_dir.as_deref(),
            normalization_cache_key.as_deref(),
        )
        .unwrap_or(1.0)
    } else {
        1.0
    };
    *state.source_bytes.lock().unwrap() = Some(bytes);
    *state.normalization_gain.lock().unwrap() = normalization_gain;
    let rebuilt = reload_current_track(state).is_ok();
    // The player reads from memory again, so stall detection can resume.
    *slot = None;
    state.streaming.store(false, Ordering::Relaxed);
    rebuilt
}

/// Recreate the player from the live stream buffer at `source_position`. Used
/// when the decoder cannot seek in place, when the track must be rebuilt on a
/// new device, and while no complete source exists. `try_seek` blocks until the
/// target's bytes arrive, which is exactly what seeking into an unbuffered range
/// has to do.
fn rebuild_from_stream(
    state: &AudioState,
    stream: &Arc<StreamBuffer>,
    source_position: f64,
    was_paused: bool,
    guard: Option<&Option<Arc<rodio::Player>>>,
) -> Result<(), String> {
    let rate = current_rate(state);
    let output_target = (source_position / rate).max(0.0);
    let normalization_gain = if state.normalization_enabled.load(Ordering::Relaxed) {
        *state.normalization_gain.lock().unwrap()
    } else {
        1.0
    };
    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let (new_player, _) = create_player_from_stream(
        stream.reader(),
        &mixer,
        vol,
        normalization_gain,
        was_paused,
        state.output_rate.load(Ordering::Relaxed),
        stream.byte_len(),
        state.playback_rate_fp.clone(),
        state.eq_params.clone(),
        state.analyser_buffer.clone(),
    )?;
    let mut seeked = source_position <= 0.0;
    if source_position > 0.0
        && new_player
            .try_seek(Duration::from_secs_f64(output_target))
            .is_ok()
    {
        seeked = true;
    }

    let mut player = state.player.lock().unwrap();
    if let Some(expected) = guard {
        let same = match (player.as_ref(), expected.as_ref()) {
            (Some(installed), Some(snapshot)) => Arc::ptr_eq(installed, snapshot),
            (None, None) => true,
            _ => false,
        };
        if !same {
            drop(player);
            // The seek that asked for this rebuild was superseded by a load/stop.
            new_player.stop();
            return Ok(());
        }
    }
    if let Some(old) = player.take() {
        old.stop();
    }
    *player = Some(Arc::new(new_player));
    // A stream without an advertised length decodes incrementally and cannot
    // seek; report the restart at 0 instead of a position the decoder is not at.
    if seeked {
        set_pos_anchor(state, source_position, output_target);
    } else {
        set_pos_anchor(state, 0.0, 0.0);
    }
    state.ended_notified.store(false, Ordering::Relaxed);
    Ok(())
}

pub async fn load_url(
    url: String,
    session_id: Option<String>,
    cache_path: Option<String>,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
    start_paused: bool,
    state: State<'_, AudioState>,
) -> Result<AudioLoadResult, String> {
    let generation = state.load_gen.load(Ordering::Relaxed);

    let client = wreq::Client::new();
    let retry_delays = [300u64, 800, 2000];
    let mut last_err = String::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut success = false;

    for attempt in 0..=retry_delays.len() {
        let mut req = client.get(&url);
        if let Some(sid) = &session_id {
            req = req.header("x-session-id", sid);
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    match resp.bytes().await {
                        Ok(b) => {
                            bytes = b.to_vec();
                            success = true;
                            break;
                        }
                        Err(e) => last_err = e.to_string(),
                    }
                } else if status.as_u16() == 429
                    || (status.as_u16() >= 500 && status.as_u16() <= 599)
                {
                    last_err = format!("HTTP {}", status);
                } else {
                    return Err(format!("HTTP {}", status));
                }
            }
            Err(e) => last_err = e.to_string(),
        }

        if attempt < retry_delays.len() {
            tokio::time::sleep(std::time::Duration::from_millis(retry_delays[attempt])).await;
            if state.load_gen.load(Ordering::Relaxed) != generation {
                return Ok(AudioLoadResult {
                    duration_secs: None,
                });
            }
        }
    }

    if !success {
        return Err(last_err);
    }
    let empty_result = AudioLoadResult {
        duration_secs: None,
    };

    if state.load_gen.load(Ordering::Relaxed) != generation {
        return Ok(empty_result);
    }

    if let Some(path) = cache_path {
        let data = bytes.clone();
        tokio::spawn(async move {
            tokio::fs::write(&path, &data).await.ok();
        });
    }

    stop_current_player(&state);

    if state.load_gen.load(Ordering::Relaxed) != generation {
        return Ok(empty_result);
    }

    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let (bytes, new_player, duration_secs, normalization_gain) = build_player_from_bytes(
        bytes,
        mixer,
        vol,
        normalization_enabled,
        normalization_cache_dir,
        normalization_cache_key,
        start_paused,
        state.output_rate.load(Ordering::Relaxed),
        state.playback_rate_fp.clone(),
        state.eq_params.clone(),
        state.analyser_buffer.clone(),
    )
    .await?;

    commit_loaded_track(&state, bytes, new_player, normalization_gain);

    Ok(AudioLoadResult { duration_secs })
}

pub fn play(state: State<'_, AudioState>) {
    // If the device errored (sleep/wake, headphone unplug), reconnect immediately
    // instead of waiting for stall detection (2s delay).
    if state.device_error.load(Ordering::Relaxed) {
        state
            .audio_tx
            .send(crate::audio::types::AudioThreadCmd::Reconnect)
            .ok();
    }
    // Always unpause so reload_current_track sees was_paused=false
    if let Ok(player) = state.player.try_lock()
        && let Some(ref player) = *player {
            player.play();
        }
}

pub fn pause(state: State<'_, AudioState>) {
    if let Ok(player) = state.player.try_lock()
        && let Some(ref player) = *player {
            player.pause();
        }
}

pub fn stop(state: State<'_, AudioState>) {
    state.has_track.store(false, Ordering::Relaxed);
    state.load_gen.fetch_add(1, Ordering::Relaxed);
    // Cancel a streaming session first: its decoder may be parked in a read and
    // must be unblocked before the player is dropped.
    if let Some(stream) = state.stream.lock().unwrap().take() {
        stream.cancel();
    }
    state.streaming.store(false, Ordering::Relaxed);
    if let Ok(mut player) = state.player.try_lock()
        && let Some(old) = player.take() {
            old.stop();
        }
    if let Ok(mut bytes) = state.source_bytes.try_lock() {
        *bytes = None;
    }
}

pub fn seek(position: f64, state: State<'_, AudioState>) -> Result<(), String> {
    suppress_ended_temporarily(&state);
    seek_to(&state, position)
}

/// Seek to `position` (source seconds), trying an in-place decoder seek first and
/// recreating the player when that fails. A bare `try_seek` silently no-ops on
/// decoders that can't seek in place, so any caller that needs the jump to actually
/// take effect (the manual slider, the A-B loop snap-back in tick.rs) must route
/// through here. Takes `&AudioState` so the tick thread can call it without `State`.
pub fn seek_to(state: &AudioState, position: f64) -> Result<(), String> {
    // `position` is in source seconds (the timeline the whole app uses). rodio's
    // try_seek operates in output time = source/rate on a speed-applied player, so
    // convert before handing it the target.
    let rate = current_rate(state);
    let output_target = (position / rate).max(0.0);
    let target = Duration::from_secs_f64(output_target);

    // Serialize seeks, but never hold the player lock across `try_seek`: on a
    // network-backed decoder the seek can sit for tens of milliseconds, and while
    // the lock was held there play/pause (try_lock) silently gave up and the
    // position readout stalled. The Arc keeps the player alive if a load swaps it.
    let _order = state.seek_lock.lock().unwrap();
    let current = state.player.lock().unwrap().clone();
    let was_paused = current.as_ref().map(|p| p.is_paused()).unwrap_or(false);

    // For position 0, always recreate the player to avoid decoder state issues
    if position > 0.0 {
        if let Some(player) = &current
            && player.try_seek(target).is_ok() {
                // Rebase only while this player is still the installed one; a
                // load or stop may have swapped it while the seek was running.
                let still_current = state
                    .player
                    .lock()
                    .unwrap()
                    .as_ref()
                    .is_some_and(|installed| Arc::ptr_eq(installed, player));
                if still_current {
                    state.ended_notified.store(false, Ordering::Relaxed);
                    set_pos_anchor(state, position, output_target);
                }
                return Ok(());
            }
    }

    let bytes = state.source_bytes.lock().unwrap().clone();
    let Some(bytes) = bytes else {
        // Streaming session without a finished download: seek by rebuilding from
        // the live buffer. Without an advertised length the decoder is reading
        // incrementally and cannot seek, so refuse rather than restarting from the
        // top; the tick thread restores the readout to the real position.
        let stream = state.stream.lock().unwrap().clone();
        let Some(stream) = stream else {
            return Err("No source to reload for seek".into());
        };
        if position > 0.0 && stream.byte_len().is_none() {
            return Err("stream is not seekable yet".into());
        }
        return rebuild_from_stream(state, &stream, position, was_paused, Some(&current));
    };

    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let normalization_gain = *state.normalization_gain.lock().unwrap();
    let (new_player, _) = create_player_from_bytes(
        &bytes,
        &mixer,
        vol,
        if normalization_enabled {
            normalization_gain
        } else {
            1.0
        },
        was_paused,
        state.output_rate.load(Ordering::Relaxed),
        state.playback_rate_fp.clone(),
        state.eq_params.clone(),
        state.analyser_buffer.clone(),
    )?;
    if position > 0.0 {
        new_player.try_seek(target).ok();
    }

    let mut player = state.player.lock().unwrap();
    let same = match (player.as_ref(), current.as_ref()) {
        (Some(installed), Some(snapshot)) => Arc::ptr_eq(installed, snapshot),
        (None, None) => true,
        _ => false,
    };
    if !same {
        drop(player);
        // A load or stop took over while the source was being rebuilt; the newer
        // load owns the engine now, so drop this player instead of clobbering it.
        new_player.stop();
        return Ok(());
    }
    if let Some(old) = player.take() {
        old.stop();
    }
    *player = Some(Arc::new(new_player));
    set_pos_anchor(state, position, output_target);
    state.ended_notified.store(false, Ordering::Relaxed);

    Ok(())
}

pub fn set_volume(volume: f64, state: State<'_, AudioState>) {
    let vol = volume_to_rodio(volume);
    *state.volume.lock().unwrap() = vol;
    if let Some(ref player) = *state.player.lock().unwrap() {
        player.set_volume(vol);
    }
}

fn clamp_playback_rate(rate: f64) -> f32 {
    (rate.clamp(0.5, 2.0)) as f32
}

pub fn set_playback_rate(rate: f64, state: State<'_, AudioState>) {
    let value = clamp_playback_rate(rate);
    {
        let player_guard = state.player.lock().unwrap();
        if let Some(ref player) = *player_guard {
            // Close the current constant-rate segment into the integrator BEFORE switching
            // speed, so source-time stays exact across the change (no re-seek, no glitch).
            let old_rate = current_rate(&state);
            let out = player.get_pos().as_secs_f64();
            let mut anchor = state.pos_anchor.lock().unwrap();
            anchor.0 += (out - anchor.1) * old_rate;
            anchor.1 = out;
        }
    }
    *state.playback_rate.lock().unwrap() = value;
    // The decode chain's SpeedSource reads this per frame, so the change is
    // audible immediately (rodio's own `set_speed` never reaches the mixer
    // mid-track — see resample.rs).
    state.playback_rate_fp.store(
        (value * crate::audio::resample::SPEED_FP_SCALE as f32) as u32,
        Ordering::Relaxed,
    );
}

pub fn get_position(state: State<'_, AudioState>) -> f64 {
    state
        .player
        .lock()
        .unwrap()
        .as_ref()
        .map(|player| source_pos(&state, player))
        .unwrap_or(0.0)
}

/// Set or clear the A-B loop region. `a`/`b` are in source seconds; a valid region
/// needs both bounds with `b` meaningfully after `a`. Anything else clears the loop.
pub fn set_ab_loop(a: Option<f64>, b: Option<f64>, state: State<'_, AudioState>) {
    let value = match (a, b) {
        (Some(a), Some(b)) if b > a + 0.05 => Some((a.max(0.0), b)),
        _ => None,
    };
    *state.ab_loop.lock().unwrap() = value;
}

pub fn set_eq(enabled: bool, gains: Vec<f64>, state: State<'_, AudioState>) {
    if let Ok(mut params) = state.eq_params.write() {
        params.enabled = enabled;
        for (index, &gain) in gains.iter().enumerate().take(EQ_BANDS) {
            params.gains[index] = gain.clamp(-12.0, 12.0);
        }
    }
}

pub fn set_normalization(enabled: bool, state: State<'_, AudioState>) {
    state
        .normalization_enabled
        .store(enabled, Ordering::Relaxed);
}

pub fn is_playing(state: State<'_, AudioState>) -> bool {
    state
        .player
        .lock()
        .unwrap()
        .as_ref()
        .map(|player| !player.is_paused() && !player.empty())
        .unwrap_or(false)
}

pub fn set_metadata(
    title: String,
    artist: String,
    cover_url: Option<String>,
    duration_secs: f64,
    state: State<'_, AudioState>,
) {
    if let Some(tx) = state.media_tx.lock().unwrap().as_ref() {
        tx.send(MediaCmd::SetMetadata {
            title,
            artist,
            cover_url,
            duration_secs,
        })
        .ok();
    }
}

pub fn set_playback_state(playing: bool, state: State<'_, AudioState>) {
    if let Some(tx) = state.media_tx.lock().unwrap().as_ref() {
        tx.send(MediaCmd::SetPlaying(playing)).ok();
    }
}

pub fn set_media_position(position: f64, state: State<'_, AudioState>) {
    if let Some(tx) = state.media_tx.lock().unwrap().as_ref() {
        tx.send(MediaCmd::SetPosition(position)).ok();
    }
}

pub async fn save_track_to_path(cache_path: String, dest_path: String) -> Result<String, String> {
    tokio::fs::copy(&cache_path, &dest_path)
        .await
        .map_err(|e| format!("Copy failed: {}", e))?;
    Ok(dest_path)
}

/* ── Hover-preview channel ───────────────────────────────────────
 * A parallel lightweight player on the shared mixer for ~15s on-hover
 * previews. Sources from an already-cached file (reusing track_cache),
 * gets its own throwaway analyser so the main player's spectrum stays
 * intact, and fades via a tick-thread volume ramp (see tick.rs). Never
 * touches the main player or plays history.
 */

fn preview_step(target: f32, fade_ms: u64) -> f32 {
    let ticks = (fade_ms as f32 / TICK_INTERVAL_MS as f32).max(1.0);
    (target / ticks).max(0.0005)
}

/// Start (or replace) the hover preview from a cached file `path` at `volume`
/// (rodio scale 0.0..2.0). The decode runs off-thread. `gen` is a monotonic token
/// from the frontend: an out-of-order (older) decode that finishes after a newer
/// hover installed its preview is dropped. There is no play-side fade-in (the
/// sample starts at target volume); fade-OUT lives in `preview_stop`.
pub async fn preview_play(
    path: String,
    volume: f64,
    r#gen: u64,
    state: State<'_, AudioState>,
) -> Result<(), String> {
    // Cheap pre-check: a hover already superseded by a newer one shouldn't pay for
    // the file read + decode (the authoritative gen check still runs post-decode).
    if r#gen < state.preview.lock().unwrap().r#gen {
        return Ok(());
    }
    let bytes = task::spawn_blocking({
        let path = path.clone();
        move || std::fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path, e))
    })
        .await
        .map_err(|e| format!("preview read task failed: {e}"))??;

    let mixer = state.mixer.lock().unwrap().clone();
    let eq_params = state.eq_params.clone();
    let output_rate = state.output_rate.load(Ordering::Relaxed);
    // Preview always plays at 1x — it has its own speed source.
    let speed = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(
        crate::audio::resample::SPEED_FP_SCALE,
    ));
    let target = (volume as f32).clamp(0.0, 2.0);
    // Own throwaway analyser buffer — the preview decoder must NOT write into the
    // main player's spectrum buffer. Normalization is skipped (gain 1.0) to keep
    // hover latency low. Build directly at the target volume so the sample is
    // audible the instant it loads (a zero-start + tick fade-in left it silent).
    let analyser = crate::audio::analyser::AnalyserBuffer::new();
    let player = task::spawn_blocking(move || {
        create_player_from_bytes(
            &bytes,
            &mixer,
            target,
            1.0,
            false,
            output_rate,
            speed,
            eq_params,
            analyser,
        )
        .map(|(player, _)| player)
    })
        .await
        .map_err(|e| format!("preview decode task failed: {e}"))??;
    player.play();

    let mut preview = state.preview.lock().unwrap();
    // A newer hover already installed its preview — drop this stale decode.
    if r#gen < preview.r#gen {
        player.stop();
        return Ok(());
    }
    if let Some(old) = preview.player.take() {
        old.stop();
    }
    preview.player = Some(player);
    preview.volume = target;
    preview.target = target;
    preview.step = 0.0;
    preview.stop_at_zero = false;
    preview.r#gen = r#gen;
    Ok(())
}

/// Stop the hover preview. `gen == 0` force-stops whatever is playing (unhover /
/// click); a non-zero `gen` is a targeted stale-stop that no-ops unless it still
/// matches the installed preview. With `fade_ms > 0` it fades out (the tick
/// thread drops the player at zero); with 0 it stops immediately.
pub fn preview_stop(fade_ms: u64, r#gen: u64, state: State<'_, AudioState>) {
    let mut preview = state.preview.lock().unwrap();
    if preview.player.is_none() {
        return;
    }
    // Targeted stop for a preview that's already been superseded — ignore.
    if r#gen != 0 && r#gen != preview.r#gen {
        return;
    }
    if fade_ms == 0 {
        if let Some(old) = preview.player.take() {
            old.stop();
        }
        preview.volume = 0.0;
        preview.target = 0.0;
        preview.step = 0.0;
        preview.stop_at_zero = false;
        return;
    }
    preview.step = preview_step(preview.volume.max(0.0005), fade_ms);
    preview.target = 0.0;
    preview.stop_at_zero = true;
}
