use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use rodio::mixer::Mixer;
use rodio::Player;

use crate::audio::analyser::AnalyserBuffer;
use crate::audio::device::open_device_sink;
use crate::audio::stream_buffer::StreamBuffer;
use crate::audio::types::{
    AudioThreadCmd, EqParams, FloatingCommentEvent, LyricsTimingLine, MediaCmd,
};

/// The endpoint the user picked for this session, remembered so a reconnect
/// (stream error, device wake) reopens it instead of falling back to the OS
/// default. `None` means "follow the OS default".
#[derive(Clone)]
pub struct OutputChoice {
    pub id: String,
    /// Friendly name, the fallback when the id stops matching.
    pub label: Option<String>,
}

pub struct LyricsTimelineState {
    pub lines: Vec<LyricsTimingLine>,
    pub active_index: Option<usize>,
}

pub struct CommentsTimelineState {
    pub comments: Vec<FloatingCommentEvent>,
    pub next_index: usize,
}

/// Lightweight hover-preview channel: a second rodio `Player` on the shared mixer
/// with its own throwaway analyser (so it never disturbs the main player's spectrum)
/// and a tick-driven linear volume ramp for fade-in/out. Never writes plays history.
pub struct PreviewState {
    pub player: Option<Player>,
    /// Currently applied volume (rodio scale 0.0..2.0).
    pub volume: f32,
    /// Target volume the tick-thread ramp is moving toward.
    pub target: f32,
    /// Per-tick volume delta magnitude (>= 0); direction derived from volume vs target.
    pub step: f32,
    /// When the ramp reaches 0, stop and drop the player (fade-out).
    pub stop_at_zero: bool,
    /// Monotonic generation of the installed preview. A newer hover bumps it; a
    /// stale stop / out-of-order decode is rejected by comparing against it so
    /// rapid hover across tiles can't clobber the surviving preview.
    pub r#gen: u64,
}

pub struct AudioState {
    /// The live player, kept behind an `Arc` so a seek can hold a handle while
    /// the lock is released. `try_seek` can block for tens of milliseconds on a
    /// network-backed decoder, and holding the mutex there stalled play, pause
    /// and the position readout (their `try_lock` calls silently gave up).
    pub player: Mutex<Option<Arc<Player>>>,
    /// Serializes seeks so the newest target still wins when two overlap. Never
    /// held while talking to the player lock except for the brief clone/swap.
    pub seek_lock: Mutex<()>,
    pub mixer: Arc<Mutex<Mixer>>,
    pub eq_params: Arc<RwLock<EqParams>>,
    pub normalization_enabled: AtomicBool,
    pub normalization_gain: Mutex<f32>,
    pub volume: Mutex<f32>,
    pub playback_rate: Mutex<f32>,
    /// Shared fixed-point playback rate (`rate * 10000`) read by the decode
    /// chain's `SpeedSource` on every frame; set alongside `playback_rate`.
    pub playback_rate_fp: Arc<AtomicU32>,
    /// Source-time position integrator `(source_anchor, output_anchor)` in seconds.
    /// rodio's get_pos() is wall-clock (output) time; source time is integrated as
    /// `source_anchor + (get_pos() - output_anchor) * rate`. The anchor is re-based on
    /// load/seek/loop and on every rate change so speed changes mid-track stay exact.
    pub pos_anchor: Mutex<(f64, f64)>,
    pub has_track: AtomicBool,
    pub ended_notified: AtomicBool,
    pub suppress_ended_until_ms: AtomicU64,
    /// Wall-clock ms until which the tick thread skips stall-detection — set around
    /// device switch/reconnect so a settling output isn't read as a dead stream.
    pub suppress_stall_until_ms: AtomicU64,
    pub device_error: Arc<AtomicBool>,
    pub device_reconnected: Arc<AtomicBool>,
    /// Sample rate the output device was opened at; the decode chain resamples
    /// every source to this rate so rodio's linear-interp mixer path is unused.
    pub output_rate: Arc<AtomicU32>,
    pub load_gen: AtomicU64,
    pub media_tx: Mutex<Option<std::sync::mpsc::Sender<MediaCmd>>>,
    pub audio_tx: std::sync::mpsc::Sender<AudioThreadCmd>,
    pub source_bytes: Mutex<Option<Vec<u8>>>,
    /// Active streaming-first-play session, if any: the download still mirroring
    /// bytes into memory for the current player. Also the identity token for
    /// "is that stream still the one this track owns" checks.
    pub stream: Mutex<Option<Arc<StreamBuffer>>>,
    /// Cheap mirror of `stream.is_some()` for the tick thread, which must not
    /// take the stream lock while holding the player lock.
    pub streaming: AtomicBool,
    pub follow_default_output: AtomicBool,
    pub last_known_default_output: Mutex<Option<String>>,
    pub lyrics_timeline: Mutex<Option<LyricsTimelineState>>,
    pub comments_timeline: Mutex<Option<CommentsTimelineState>>,
    /// A-B loop region `(a, b)` in **source seconds** (a < b). When set, playback
    /// jumps back to `a` once it crosses `b` (see tick.rs). None = disabled.
    pub ab_loop: Mutex<Option<(f64, f64)>>,
    pub analyser_buffer: Arc<AnalyserBuffer>,
    pub preview: Mutex<PreviewState>,
    /// Endpoint to reopen on reconnect; written by the output thread when a
    /// switch succeeds and cleared when it has to fall back to the system default.
    pub manual_output: Arc<Mutex<Option<OutputChoice>>>,
    /// Set from `setup` so the output thread can write device decisions to
    /// desktop.log (it starts before an AppHandle exists).
    pub app_handle: Arc<OnceLock<crate::rt::AppHandle>>,
}

impl AudioState {
    pub fn set_app_handle(&self, handle: crate::rt::AppHandle) {
        let _ = self.app_handle.set(handle);
    }
}

pub fn init() -> AudioState {
    let (mixer_tx, mixer_rx) = std::sync::mpsc::channel::<Arc<Mutex<Mixer>>>();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AudioThreadCmd>();
    let device_error_flag = Arc::new(AtomicBool::new(false));
    let reconnected_flag = Arc::new(AtomicBool::new(false));
    let output_rate = Arc::new(AtomicU32::new(0));
    // Shared with the output thread: it starts before `setup` exists, so it reads
    // the handle lazily to put device decisions into desktop.log.
    let app_handle_slot: Arc<OnceLock<crate::rt::AppHandle>> = Arc::new(OnceLock::new());
    let manual_output: Arc<Mutex<Option<OutputChoice>>> = Arc::new(Mutex::new(None));

    let cmd_tx_for_thread = cmd_tx.clone();
    let reconnected_for_thread = reconnected_flag.clone();
    let error_flag_for_thread = device_error_flag.clone();
    let output_rate_for_thread = output_rate.clone();
    let app_handle_for_thread = app_handle_slot.clone();
    let manual_output_for_thread = manual_output.clone();
    std::thread::Builder::new()
        .name("audio-output".into())
        .spawn(move || {
            let cmd_tx = cmd_tx_for_thread;
            let reconnected = reconnected_for_thread;
            let error_flag = error_flag_for_thread;
            let output_rate = output_rate_for_thread;
            let manual_output = manual_output_for_thread;
            // A silent fallback to the system default is the failure mode this
            // path used to hide; every open decision goes to desktop.log.
            let log_device = |level: &str, message: String| {
                if let Some(app) = app_handle_for_thread.get() {
                    crate::app::diagnostics::log_native(app, level, &message);
                }
                match level {
                    "WARN" | "ERROR" => eprintln!("{message}"),
                    _ => println!("{message}"),
                }
            };

            let mut device_sink =
                open_device_sink(None, None, &cmd_tx, &error_flag, &output_rate)
                    .expect("no audio output device");
            log_device(
                "INFO",
                "[Audio] output opened: system default".to_string(),
            );
            let shared_mixer = Arc::new(Mutex::new(device_sink.mixer().clone()));
            mixer_tx.send(shared_mixer.clone()).ok();

            loop {
                match cmd_rx.recv() {
                    Ok(AudioThreadCmd::SwitchDevice { name, label, reply }) => {
                        drop(device_sink);

                        match open_device_sink(
                            name.as_deref(),
                            label.as_deref(),
                            &cmd_tx,
                            &error_flag,
                            &output_rate,
                        ) {
                            Ok(new_sink) => {
                                let mixer = new_sink.mixer().clone();
                                *shared_mixer.lock().unwrap() = mixer.clone();
                                device_sink = new_sink;
                                *manual_output.lock().unwrap() = name.clone().map(|id| OutputChoice {
                                    id,
                                    label: label.clone(),
                                });
                                log_device(
                                    "INFO",
                                    match &name {
                                        Some(id) => format!("[Audio] output opened: '{id}'"),
                                        None => {
                                            "[Audio] output opened: system default".to_string()
                                        }
                                    },
                                );
                                reply.send(Ok(mixer)).ok();
                            }
                            Err(error) => {
                                log_device(
                                    "WARN",
                                    format!("[Audio] output switch failed: {error}"),
                                );
                                device_sink =
                                    open_device_sink(None, None, &cmd_tx, &error_flag, &output_rate)
                                        .expect("no audio output device");
                                *shared_mixer.lock().unwrap() = device_sink.mixer().clone();
                                // The requested endpoint is gone for good: a later
                                // reconnect must not keep chasing it.
                                *manual_output.lock().unwrap() = None;
                                reply.send(Err(error)).ok();
                            }
                        }
                    }
                    Ok(AudioThreadCmd::Reconnect) => {
                        log_device(
                            "INFO",
                            "[Audio] output invalidated, reconnecting...".to_string(),
                        );
                        std::thread::sleep(Duration::from_millis(500));

                        // Reopen what the user picked, not the OS default: a
                        // stream error used to reset a manual device silently.
                        let wanted = manual_output.lock().unwrap().clone();
                        drop(device_sink);
                        let (opened, target) = match wanted.as_ref() {
                            Some(choice) => (
                                open_device_sink(
                                    Some(&choice.id),
                                    choice.label.as_deref(),
                                    &cmd_tx,
                                    &error_flag,
                                    &output_rate,
                                ),
                                format!("saved device '{}'", choice.id),
                            ),
                            None => (
                                open_device_sink(None, None, &cmd_tx, &error_flag, &output_rate),
                                "system default".to_string(),
                            ),
                        };
                        match opened {
                            Ok(new_sink) => {
                                *shared_mixer.lock().unwrap() = new_sink.mixer().clone();
                                device_sink = new_sink;
                                reconnected.store(true, std::sync::atomic::Ordering::Release);
                                log_device("INFO", format!("[Audio] reconnected on {target}"));
                            }
                            Err(error) => {
                                log_device(
                                    "WARN",
                                    format!("[Audio] reconnect on {target} failed: {error}"),
                                );
                                std::thread::sleep(Duration::from_secs(1));
                                *manual_output.lock().unwrap() = None;
                                device_sink =
                                    open_device_sink(None, None, &cmd_tx, &error_flag, &output_rate)
                                        .expect("no audio output device");
                                *shared_mixer.lock().unwrap() = device_sink.mixer().clone();
                                reconnected.store(true, std::sync::atomic::Ordering::Release);
                                log_device(
                                    "INFO",
                                    "[Audio] reconnected on the system default".to_string(),
                                );
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
        })
        .expect("failed to spawn audio thread");

    let shared_mixer = mixer_rx.recv().expect("audio thread failed to init");

    AudioState {
        player: Mutex::new(None),
        seek_lock: Mutex::new(()),
        mixer: shared_mixer,
        eq_params: Arc::new(RwLock::new(EqParams::default())),
        normalization_enabled: AtomicBool::new(true),
        normalization_gain: Mutex::new(1.0),
        volume: Mutex::new(0.25),
        playback_rate: Mutex::new(1.0),
        playback_rate_fp: Arc::new(AtomicU32::new(crate::audio::resample::SPEED_FP_SCALE)),
        pos_anchor: Mutex::new((0.0, 0.0)),
        has_track: AtomicBool::new(false),
        ended_notified: AtomicBool::new(false),
        suppress_ended_until_ms: AtomicU64::new(0),
        suppress_stall_until_ms: AtomicU64::new(0),
        device_error: device_error_flag,
        device_reconnected: reconnected_flag,
        output_rate,
        load_gen: AtomicU64::new(0),
        media_tx: Mutex::new(None),
        audio_tx: cmd_tx,
        source_bytes: Mutex::new(None),
        stream: Mutex::new(None),
        streaming: AtomicBool::new(false),
        follow_default_output: AtomicBool::new(true),
        last_known_default_output: Mutex::new(None),
        lyrics_timeline: Mutex::new(None),
        comments_timeline: Mutex::new(None),
        ab_loop: Mutex::new(None),
        analyser_buffer: AnalyserBuffer::new(),
        manual_output,
        app_handle: app_handle_slot,
        preview: Mutex::new(PreviewState {
            player: None,
            volume: 0.0,
            target: 0.0,
            step: 0.0,
            stop_at_zero: false,
            r#gen: 0,
        }),
    }
}
