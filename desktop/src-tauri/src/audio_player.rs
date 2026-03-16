use std::io::{BufReader, Cursor};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use biquad::{Biquad, Coefficients, DirectForm1, Hertz, ToHertz, Type, Q_BUTTERWORTH_F64};
use rodio::mixer::Mixer;
use rodio::source::SeekError;
use rodio::stream::DeviceSinkBuilder;
use rodio::{Decoder, Player, Source};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata as SmtcMetadata, MediaPlayback, MediaPosition,
    PlatformConfig,
};
use tauri::{AppHandle, Emitter, Manager};

/* ── Constants ─────────────────────────────────────────────── */

const EQ_BANDS: usize = 10;
const EQ_FREQS: [f64; EQ_BANDS] = [
    32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0,
];
const EQ_Q: f64 = 1.414; // ~1 octave bandwidth for peaking filters
const TICK_INTERVAL_MS: u64 = 100;

type ChannelCount = NonZero<u16>;
type SampleRate = NonZero<u32>;

/* ── EQ Parameters (shared between audio thread and commands) ─ */

pub struct EqParams {
    pub enabled: bool,
    pub gains: [f64; EQ_BANDS], // dB, -12 to +12
}

impl Default for EqParams {
    fn default() -> Self {
        Self {
            enabled: false,
            gains: [0.0; EQ_BANDS],
        }
    }
}

/* ── EQ Source wrapper ─────────────────────────────────────── */

struct EqSource<S: Source<Item = f32>> {
    source: S,
    params: Arc<RwLock<EqParams>>,
    filters_l: [DirectForm1<f64>; EQ_BANDS],
    filters_r: [DirectForm1<f64>; EQ_BANDS],
    channels: ChannelCount,
    sample_rate: SampleRate,
    current_channel: u16,
    // Cached gains to detect changes and recompute coefficients
    cached_gains: [f64; EQ_BANDS],
    cached_enabled: bool,
}

impl<S: Source<Item = f32>> EqSource<S> {
    fn new(source: S, params: Arc<RwLock<EqParams>>) -> Self {
        let sample_rate = source.sample_rate();
        let channels = source.channels();
        let fs: Hertz<f64> = (sample_rate.get() as f64).hz();

        let make_filters = || {
            std::array::from_fn(|i| {
                let filter_type = if i == 0 {
                    Type::LowShelf(0.0)
                } else if i == EQ_BANDS - 1 {
                    Type::HighShelf(0.0)
                } else {
                    Type::PeakingEQ(0.0)
                };
                let q = if i == 0 || i == EQ_BANDS - 1 {
                    Q_BUTTERWORTH_F64
                } else {
                    EQ_Q
                };
                let coeffs =
                    Coefficients::<f64>::from_params(filter_type, fs, EQ_FREQS[i].hz(), q)
                        .unwrap();
                DirectForm1::<f64>::new(coeffs)
            })
        };

        Self {
            source,
            params,
            filters_l: make_filters(),
            filters_r: make_filters(),
            channels,
            sample_rate,
            current_channel: 0,
            cached_gains: [0.0; EQ_BANDS],
            cached_enabled: false,
        }
    }

    fn update_coefficients(&mut self, gains: &[f64; EQ_BANDS]) {
        let fs: Hertz<f64> = (self.sample_rate.get() as f64).hz();
        for i in 0..EQ_BANDS {
            if (gains[i] - self.cached_gains[i]).abs() < 0.01 {
                continue;
            }
            let filter_type = if i == 0 {
                Type::LowShelf(gains[i])
            } else if i == EQ_BANDS - 1 {
                Type::HighShelf(gains[i])
            } else {
                Type::PeakingEQ(gains[i])
            };
            let q = if i == 0 || i == EQ_BANDS - 1 {
                Q_BUTTERWORTH_F64
            } else {
                EQ_Q
            };
            if let Ok(coeffs) =
                Coefficients::<f64>::from_params(filter_type, fs, EQ_FREQS[i].hz(), q)
            {
                self.filters_l[i] = DirectForm1::<f64>::new(coeffs);
                self.filters_r[i] = DirectForm1::<f64>::new(coeffs);
            }
        }
        self.cached_gains = *gains;
    }
}

impl<S: Source<Item = f32>> Iterator for EqSource<S> {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        let sample = self.source.next()?;
        let ch = self.current_channel;
        self.current_channel = (ch + 1) % self.channels.get();

        // Read EQ params (non-blocking — skip if locked)
        let snapshot = self.params.try_read().ok().map(|p| (p.enabled, p.gains));
        if let Some((enabled, gains)) = snapshot {
            if enabled != self.cached_enabled || gains != self.cached_gains {
                if enabled {
                    self.update_coefficients(&gains);
                }
                self.cached_enabled = enabled;
            }
        }

        if !self.cached_enabled {
            return Some(sample);
        }

        let mut out = sample as f64;
        let filters = if ch == 0 {
            &mut self.filters_l
        } else {
            &mut self.filters_r
        };
        for f in filters.iter_mut() {
            out = Biquad::run(f, out);
        }
        Some(out.clamp(-1.0, 1.0) as f32)
    }
}

impl<S: Source<Item = f32>> Source for EqSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.source.try_seek(pos)
    }
}

/* ── Audio State (managed by Tauri) ────────────────────────── */

/// Messages sent to the media controls thread
enum MediaCmd {
    SetMetadata {
        title: String,
        artist: String,
        cover_url: Option<String>,
        duration_secs: f64,
    },
    SetPlaying(bool),
    SetPosition(f64),
}

pub struct AudioState {
    player: Mutex<Option<Player>>,
    eq_params: Arc<RwLock<EqParams>>,
    volume: Mutex<f32>, // 0.0 - 2.0
    has_track: AtomicBool,
    ended_notified: AtomicBool,
    media_tx: Mutex<Option<std::sync::mpsc::Sender<MediaCmd>>>,
}

// Keep MixerDeviceSink alive globally; Mixer (Clone + Send) is extracted for creating Players
static MIXER: OnceLock<Mixer> = OnceLock::new();

pub fn init() -> AudioState {
    // Spawn audio output on a dedicated thread (MixerDeviceSink may be !Send on some platforms)
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("audio-output".into())
        .spawn(move || {
            let mut device_sink =
                DeviceSinkBuilder::open_default_sink().expect("no audio output device");
            device_sink.log_on_drop(false);
            let mixer = device_sink.mixer().clone();
            tx.send(mixer).ok();
            // Keep device_sink alive — dropping it stops all audio
            let _keep = device_sink;
            loop {
                std::thread::park();
            }
        })
        .expect("failed to spawn audio thread");

    let mixer = rx.recv().expect("audio thread failed to init");
    MIXER.set(mixer).ok();

    AudioState {
        player: Mutex::new(None),
        eq_params: Arc::new(RwLock::new(EqParams::default())),
        volume: Mutex::new(0.25), // 50/200
        has_track: AtomicBool::new(false),
        ended_notified: AtomicBool::new(false),
        media_tx: Mutex::new(None),
    }
}

/// Start background thread that emits position ticks and track-end events
pub fn start_tick_emitter(app: &AppHandle) {
    let handle = app.clone();
    std::thread::Builder::new()
        .name("audio-tick".into())
        .spawn(move || loop {
            std::thread::sleep(Duration::from_millis(TICK_INTERVAL_MS));
            let state = handle.state::<AudioState>();

            if !state.has_track.load(Ordering::Relaxed) {
                continue;
            }

            let player = state.player.lock().unwrap();
            if let Some(ref p) = *player {
                if p.empty() {
                    // Track ended
                    if !state.ended_notified.swap(true, Ordering::Relaxed) {
                        handle.emit("audio:ended", ()).ok();
                    }
                } else {
                    let pos = p.get_pos().as_secs_f64();
                    handle.emit("audio:tick", pos).ok();
                }
            }
        })
        .expect("failed to spawn tick thread");
}

/// Start media controls (MPRIS on Linux, SMTC on Windows) on a dedicated thread
pub fn start_media_controls(app: &AppHandle) {
    let handle = app.clone();
    let (tx, rx) = std::sync::mpsc::channel::<MediaCmd>();

    // Store sender in AudioState
    let state = app.state::<AudioState>();
    *state.media_tx.lock().unwrap() = Some(tx);

    std::thread::Builder::new()
        .name("media-controls".into())
        .spawn(move || {
            #[cfg(not(target_os = "windows"))]
            let hwnd = None;

            #[cfg(target_os = "windows")]
            let hwnd = {
                use tauri::Manager;
                handle
                    .get_webview_window("main")
                    .and_then(|w| {
                        use raw_window_handle::HasWindowHandle;
                        w.window_handle().ok().and_then(|wh| match wh.as_raw() {
                            raw_window_handle::RawWindowHandle::Win32(h) => {
                                Some(h.hwnd.get() as *mut std::ffi::c_void)
                            }
                            _ => None,
                        })
                    })
            };

            let config = PlatformConfig {
                display_name: "SoundCloud Desktop",
                dbus_name: "soundcloud_desktop",
                hwnd,
            };

            let mut controls = match MediaControls::new(config) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[MediaControls] Failed to create: {:?}", e);
                    return;
                }
            };

            let event_handle = handle.clone();
            controls
                .attach(move |event: MediaControlEvent| {
                    match event {
                        MediaControlEvent::Play => {
                            event_handle.emit("media:play", ()).ok();
                        }
                        MediaControlEvent::Pause => {
                            event_handle.emit("media:pause", ()).ok();
                        }
                        MediaControlEvent::Toggle => {
                            event_handle.emit("media:toggle", ()).ok();
                        }
                        MediaControlEvent::Next => {
                            event_handle.emit("media:next", ()).ok();
                        }
                        MediaControlEvent::Previous => {
                            event_handle.emit("media:prev", ()).ok();
                        }
                        MediaControlEvent::SetPosition(MediaPosition(pos)) => {
                            event_handle.emit("media:seek", pos.as_secs_f64()).ok();
                        }
                        MediaControlEvent::Seek(dir) => {
                            let offset = match dir {
                                souvlaki::SeekDirection::Forward => 10.0,
                                souvlaki::SeekDirection::Backward => -10.0,
                            };
                            event_handle.emit("media:seek-relative", offset).ok();
                        }
                        _ => {}
                    }
                })
                .ok();

            // Process commands from main thread
            loop {
                match rx.recv() {
                    Ok(MediaCmd::SetMetadata {
                        title,
                        artist,
                        cover_url,
                        duration_secs,
                    }) => {
                        controls
                            .set_metadata(SmtcMetadata {
                                title: Some(&title),
                                artist: Some(&artist),
                                cover_url: cover_url.as_deref(),
                                duration: if duration_secs > 0.0 {
                                    Some(Duration::from_secs_f64(duration_secs))
                                } else {
                                    None
                                },
                                ..Default::default()
                            })
                            .ok();
                    }
                    Ok(MediaCmd::SetPlaying(playing)) => {
                        let state = handle.state::<AudioState>();
                        let pos = state
                            .player
                            .lock()
                            .unwrap()
                            .as_ref()
                            .map(|p| p.get_pos())
                            .unwrap_or_default();
                        let progress = Some(MediaPosition(pos));
                        let playback = if playing {
                            MediaPlayback::Playing { progress }
                        } else {
                            MediaPlayback::Paused { progress }
                        };
                        controls.set_playback(playback).ok();
                    }
                    Ok(MediaCmd::SetPosition(secs)) => {
                        // Just update position without changing play state
                        let state = handle.state::<AudioState>();
                        let is_playing = state
                            .player
                            .lock()
                            .unwrap()
                            .as_ref()
                            .map(|p| !p.is_paused() && !p.empty())
                            .unwrap_or(false);
                        let progress = Some(MediaPosition(Duration::from_secs_f64(secs)));
                        let playback = if is_playing {
                            MediaPlayback::Playing { progress }
                        } else {
                            MediaPlayback::Paused { progress }
                        };
                        controls.set_playback(playback).ok();
                    }
                    Err(_) => break, // Channel closed
                }
            }
        })
        .expect("failed to spawn media-controls thread");
}

/* ── Tauri Commands ────────────────────────────────────────── */

fn volume_to_rodio(v: f64) -> f32 {
    // Frontend: 0-200, where 100 = normal. rodio: 0.0 = silent, 1.0 = normal
    (v / 100.0).min(2.0).max(0.0) as f32
}

/// Load and play audio from a file path
#[tauri::command]
pub fn audio_load_file(path: String, state: tauri::State<'_, AudioState>) -> Result<(), String> {
    let mixer = MIXER.get().ok_or("audio not initialized")?;

    let file =
        std::fs::File::open(&path).map_err(|e| format!("Failed to open {}: {}", path, e))?;
    let source =
        Decoder::new(BufReader::new(file)).map_err(|e| format!("Failed to decode: {}", e))?;

    let eq_source = EqSource::new(source, state.eq_params.clone());

    let new_player = Player::connect_new(mixer);
    let vol = *state.volume.lock().unwrap();
    new_player.set_volume(vol);
    new_player.append(eq_source);

    // Replace old player
    let mut player = state.player.lock().unwrap();
    if let Some(old) = player.take() {
        old.stop();
    }
    *player = Some(new_player);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);

    Ok(())
}

/// Load and play audio from a URL (downloads fully, optionally caches)
#[tauri::command]
pub async fn audio_load_url(
    url: String,
    session_id: Option<String>,
    cache_path: Option<String>,
    state: tauri::State<'_, AudioState>,
) -> Result<(), String> {
    // Download
    let client = reqwest::Client::new();
    let mut req = client.get(&url);
    if let Some(sid) = &session_id {
        req = req.header("x-session-id", sid);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().await.map_err(|e| e.to_string())?.to_vec();

    // Cache in background
    if let Some(path) = cache_path {
        let data = bytes.clone();
        tokio::spawn(async move {
            tokio::fs::write(&path, &data).await.ok();
        });
    }

    // Decode and play
    let mixer = MIXER.get().ok_or("audio not initialized")?;
    let cursor = Cursor::new(bytes);
    let source = Decoder::new(cursor).map_err(|e| format!("Failed to decode: {}", e))?;
    let eq_source = EqSource::new(source, state.eq_params.clone());

    let new_player = Player::connect_new(mixer);
    let vol = *state.volume.lock().unwrap();
    new_player.set_volume(vol);
    new_player.append(eq_source);

    let mut player = state.player.lock().unwrap();
    if let Some(old) = player.take() {
        old.stop();
    }
    *player = Some(new_player);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);

    Ok(())
}

#[tauri::command]
pub fn audio_play(state: tauri::State<'_, AudioState>) {
    if let Some(ref p) = *state.player.lock().unwrap() {
        p.play();
    }
}

#[tauri::command]
pub fn audio_pause(state: tauri::State<'_, AudioState>) {
    if let Some(ref p) = *state.player.lock().unwrap() {
        p.pause();
    }
}

#[tauri::command]
pub fn audio_stop(state: tauri::State<'_, AudioState>) {
    let mut player = state.player.lock().unwrap();
    if let Some(old) = player.take() {
        old.stop();
    }
    state.has_track.store(false, Ordering::Relaxed);
}

#[tauri::command]
pub fn audio_seek(position: f64, state: tauri::State<'_, AudioState>) -> Result<(), String> {
    if let Some(ref p) = *state.player.lock().unwrap() {
        p.try_seek(Duration::from_secs_f64(position))
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn audio_set_volume(volume: f64, state: tauri::State<'_, AudioState>) {
    let vol = volume_to_rodio(volume);
    *state.volume.lock().unwrap() = vol;
    if let Some(ref p) = *state.player.lock().unwrap() {
        p.set_volume(vol);
    }
}

#[tauri::command]
pub fn audio_get_position(state: tauri::State<'_, AudioState>) -> f64 {
    state
        .player
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.get_pos().as_secs_f64())
        .unwrap_or(0.0)
}

#[tauri::command]
pub fn audio_set_eq(enabled: bool, gains: Vec<f64>, state: tauri::State<'_, AudioState>) {
    if let Ok(mut params) = state.eq_params.write() {
        params.enabled = enabled;
        for (i, &g) in gains.iter().enumerate().take(EQ_BANDS) {
            params.gains[i] = g.clamp(-12.0, 12.0);
        }
    }
}

#[tauri::command]
pub fn audio_is_playing(state: tauri::State<'_, AudioState>) -> bool {
    state
        .player
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| !p.is_paused() && !p.empty())
        .unwrap_or(false)
}

#[tauri::command]
pub fn audio_set_metadata(
    title: String,
    artist: String,
    cover_url: Option<String>,
    duration_secs: f64,
    state: tauri::State<'_, AudioState>,
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

#[tauri::command]
pub fn audio_set_playback_state(playing: bool, state: tauri::State<'_, AudioState>) {
    if let Some(tx) = state.media_tx.lock().unwrap().as_ref() {
        tx.send(MediaCmd::SetPlaying(playing)).ok();
    }
}

#[tauri::command]
pub fn audio_set_media_position(position: f64, state: tauri::State<'_, AudioState>) {
    if let Some(tx) = state.media_tx.lock().unwrap().as_ref() {
        tx.send(MediaCmd::SetPosition(position)).ok();
    }
}