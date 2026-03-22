use std::io::Cursor;
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
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
const NORMALIZATION_ANALYSIS_SAMPLES: usize = 48_000 * 2 * 30; // ~30s stereo at 48kHz
const NORMALIZATION_TARGET_RMS: f64 = 0.14;
const NORMALIZATION_TARGET_PEAK: f64 = 0.95;
const NORMALIZATION_MAX_BOOST_DB: f64 = 9.0;
const NORMALIZATION_MAX_ATTENUATION_DB: f64 = -8.0;
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

struct GainSource<S: Source<Item = f32>> {
    source: S,
    gain: f32,
}

impl<S: Source<Item = f32>> GainSource<S> {
    fn new(source: S, gain: f32) -> Self {
        Self { source, gain }
    }
}

impl<S: Source<Item = f32>> Iterator for GainSource<S> {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        self.source
            .next()
            .map(|sample| (sample * self.gain).clamp(-1.0, 1.0))
    }
}

impl<S: Source<Item = f32>> Source for GainSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.source.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.source.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.source.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.source.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.source.try_seek(pos)
    }
}

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

/* ── OGG/Opus Source ───────────────────────────────────────── */

struct OpusSource {
    reader: ogg::reading::PacketReader<Cursor<Vec<u8>>>,
    decoder: audiopus::coder::Decoder,
    channels: ChannelCount,
    buffer: Vec<f32>,
    buf_pos: usize,
    serial: u32,
    pre_skip: usize,
    samples_skipped: usize,
}

impl OpusSource {
    fn new(data: Vec<u8>) -> Result<Self, String> {
        let mut reader = ogg::reading::PacketReader::new(Cursor::new(data));

        let head_pkt = reader
            .read_packet()
            .map_err(|e| format!("OGG read error: {}", e))?
            .ok_or("No OpusHead packet")?;

        let head = &head_pkt.data;
        if head.len() < 19 || &head[..8] != b"OpusHead" {
            return Err("Invalid OpusHead".into());
        }

        let serial = head_pkt.stream_serial();
        let ch_count = head[9];
        let pre_skip = u16::from_le_bytes([head[10], head[11]]) as usize;

        let opus_ch = if ch_count == 1 {
            audiopus::Channels::Mono
        } else {
            audiopus::Channels::Stereo
        };

        // Skip OpusTags
        reader
            .read_packet()
            .map_err(|e| format!("OGG read error: {}", e))?;

        let decoder = audiopus::coder::Decoder::new(audiopus::SampleRate::Hz48000, opus_ch)
            .map_err(|e| format!("Opus decoder error: {:?}", e))?;

        let ch = if ch_count == 1 { 1u16 } else { 2u16 };

        Ok(Self {
            reader,
            decoder,
            channels: NonZero::new(ch).unwrap(),
            buffer: Vec::new(),
            buf_pos: 0,
            serial,
            pre_skip: pre_skip * ch as usize,
            samples_skipped: 0,
        })
    }

    fn decode_next_packet(&mut self) -> bool {
        loop {
            match self.reader.read_packet() {
                Ok(Some(pkt)) => {
                    if pkt.data.is_empty() {
                        continue;
                    }
                    let ch = self.channels.get() as usize;
                    let mut buf = vec![0f32; 5760 * ch];
                    match self
                        .decoder
                        .decode_float(Some(&pkt.data), &mut buf, false)
                    {
                        Ok(samples_per_ch) => {
                            let total = samples_per_ch * ch;
                            buf.truncate(total);

                            if self.samples_skipped < self.pre_skip {
                                let skip = (self.pre_skip - self.samples_skipped).min(total);
                                self.samples_skipped += skip;
                                if skip >= total {
                                    continue;
                                }
                                self.buffer = buf[skip..].to_vec();
                            } else {
                                self.buffer = buf;
                            }
                            self.buf_pos = 0;
                            return true;
                        }
                        Err(_) => continue,
                    }
                }
                _ => return false,
            }
        }
    }
}

impl Iterator for OpusSource {
    type Item = f32;

    #[inline]
    fn next(&mut self) -> Option<f32> {
        if self.buf_pos >= self.buffer.len() {
            if !self.decode_next_packet() {
                return None;
            }
        }
        let sample = self.buffer[self.buf_pos];
        self.buf_pos += 1;
        Some(sample)
    }
}

impl Source for OpusSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        self.channels
    }
    fn sample_rate(&self) -> SampleRate {
        NonZero::new(48000).unwrap()
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        let target_gp = (pos.as_secs_f64() * 48000.0) as u64;

        match self.reader.seek_absgp(Some(self.serial), target_gp) {
            Ok(_) => {
                let opus_ch = if self.channels.get() == 1 {
                    audiopus::Channels::Mono
                } else {
                    audiopus::Channels::Stereo
                };
                self.decoder =
                    audiopus::coder::Decoder::new(audiopus::SampleRate::Hz48000, opus_ch)
                        .map_err(|_| SeekError::NotSupported {
                            underlying_source: "opus decoder reinit failed",
                        })?;
                self.buffer.clear();
                self.buf_pos = 0;
                self.samples_skipped = self.pre_skip;
                Ok(())
            }
            Err(_) => Err(SeekError::NotSupported {
                underlying_source: "ogg seek failed",
            }),
        }
    }
}

/* ── Decode helper ─────────────────────────────────────────── */

fn normalization_gain_from_samples<I>(samples: I) -> f32
where
    I: IntoIterator<Item = f32>,
{
    let mut sum_sq = 0.0f64;
    let mut peak = 0.0f64;
    let mut count = 0usize;

    for sample in samples.into_iter().take(NORMALIZATION_ANALYSIS_SAMPLES) {
        let v = sample as f64;
        let abs = v.abs();
        peak = peak.max(abs);
        sum_sq += v * v;
        count += 1;
    }

    if count == 0 {
        return 1.0;
    }

    let rms = (sum_sq / count as f64).sqrt().max(1e-6);
    let target_gain = NORMALIZATION_TARGET_RMS / rms;
    let peak_safe_gain = if peak > 0.0 {
        NORMALIZATION_TARGET_PEAK / peak
    } else {
        target_gain
    };

    let max_boost = 10f64.powf(NORMALIZATION_MAX_BOOST_DB / 20.0);
    let max_attenuation = 10f64.powf(NORMALIZATION_MAX_ATTENUATION_DB / 20.0);
    let gain = target_gain
        .min(peak_safe_gain)
        .clamp(max_attenuation, max_boost);

    if (gain - 1.0).abs() < 0.05 {
        1.0
    } else {
        gain as f32
    }
}

fn create_player_from_bytes(
    bytes: &[u8],
    mixer: &Mixer,
    volume: f32,
    normalization_enabled: bool,
    eq_params: Arc<RwLock<EqParams>>,
) -> Result<(Player, Option<f64>), String> {
    let player = Player::connect_new(mixer);
    player.set_volume(volume);

    let duration;
    if let Ok(source) = Decoder::new(Cursor::new(bytes.to_vec())) {
        let gain = if normalization_enabled {
            normalization_gain_from_samples(source)
        } else {
            1.0
        };
        let source = Decoder::new(Cursor::new(bytes.to_vec()))
            .map_err(|e| format!("Failed to decode: {}", e))?;
        duration = source.total_duration().map(|d| d.as_secs_f64());
        player.append(EqSource::new(GainSource::new(source, gain), eq_params));
    } else {
        let gain = if normalization_enabled {
            normalization_gain_from_samples(
                OpusSource::new(bytes.to_vec()).map_err(|e| format!("Failed to decode: {}", e))?,
            )
        } else {
            1.0
        };
        let source = OpusSource::new(bytes.to_vec())
            .map_err(|e| format!("Failed to decode: {}", e))?;
        duration = source.total_duration().map(|d| d.as_secs_f64());
        player.append(EqSource::new(GainSource::new(source, gain), eq_params));
    }

    Ok((player, duration))
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

/// Command sent to the audio output thread (which owns MixerDeviceSink)
enum AudioThreadCmd {
    SwitchDevice {
        name: Option<String>,
        reply: std::sync::mpsc::Sender<Result<Mixer, String>>,
    },
    /// Auto-reconnect when the audio device is invalidated (e.g. BT profile switch)
    Reconnect,
}

pub struct AudioState {
    player: Mutex<Option<Player>>,
    mixer: Arc<Mutex<Mixer>>,
    eq_params: Arc<RwLock<EqParams>>,
    normalization_enabled: AtomicBool,
    volume: Mutex<f32>, // 0.0 - 2.0
    has_track: AtomicBool,
    ended_notified: AtomicBool,
    /// Set by error callback when stream breaks, cleared after reconnect completes
    device_error: Arc<AtomicBool>,
    /// Set by audio thread on device reconnect (e.g. BT profile switch), cleared by tick emitter
    device_reconnected: Arc<AtomicBool>,
    load_gen: AtomicU64,
    media_tx: Mutex<Option<std::sync::mpsc::Sender<MediaCmd>>>,
    audio_tx: std::sync::mpsc::Sender<AudioThreadCmd>,
    /// Saved source bytes for seek fallback (reload + seek forward)
    source_bytes: Mutex<Option<Vec<u8>>>,
}

fn open_device_sink(
    device_id: Option<&str>,
    reconnect_tx: &std::sync::mpsc::Sender<AudioThreadCmd>,
    error_flag: &Arc<AtomicBool>,
) -> Result<rodio::stream::MixerDeviceSink, String> {
    use cpal::traits::{DeviceTrait, HostTrait};

    // Error callback: on stream error (e.g. BT profile switch → AUDCLNT_E_DEVICE_INVALIDATED),
    // signal audio thread to reconnect. AtomicBool prevents spamming.
    let sent = Arc::new(AtomicBool::new(false));
    let sent_clone = sent.clone();
    let tx = reconnect_tx.clone();
    let err_flag = error_flag.clone();
    let error_cb = move |err: cpal::StreamError| {
        eprintln!("[audio] stream error: {err}");
        err_flag.store(true, Ordering::Relaxed);
        if !sent_clone.swap(true, Ordering::Relaxed) {
            tx.send(AudioThreadCmd::Reconnect).ok();
        }
    };

    if let Some(id) = device_id {
        let host = cpal::default_host();
        if let Ok(devices) = host.output_devices() {
            for dev in devices {
                if dev.id().ok().map(|d| d.to_string()).as_deref() == Some(id) {
                    let mut sink = DeviceSinkBuilder::from_device(dev)
                        .map_err(|e| format!("Failed to open device '{}': {}", id, e))?
                        .with_error_callback(error_cb)
                        .open_stream()
                        .map_err(|e| format!("Failed to open device '{}': {}", id, e))?;
                    sink.log_on_drop(false);
                    return Ok(sink);
                }
            }
        }
        return Err(format!("Device '{}' not found", id));
    }

    let mut sink = DeviceSinkBuilder::from_default_device()
        .map_err(|e| format!("No audio output: {}", e))?
        .with_error_callback(error_cb)
        .open_stream()
        .map_err(|e| format!("No audio output: {}", e))?;
    sink.log_on_drop(false);
    Ok(sink)
}

pub fn init() -> AudioState {
    // Spawn audio output on a dedicated thread (MixerDeviceSink may be !Send on some platforms)
    let (mixer_tx, mixer_rx) = std::sync::mpsc::channel::<Arc<Mutex<Mixer>>>();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<AudioThreadCmd>();
    let device_error_flag = Arc::new(AtomicBool::new(false));
    let reconnected_flag = Arc::new(AtomicBool::new(false));

    let cmd_tx_for_thread = cmd_tx.clone();
    let reconnected_for_thread = reconnected_flag.clone();
    let error_flag_for_thread = device_error_flag.clone();
    std::thread::Builder::new()
        .name("audio-output".into())
        .spawn(move || {
            let cmd_tx = cmd_tx_for_thread;
            let reconnected = reconnected_for_thread;
            let error_flag = error_flag_for_thread;
            let mut device_sink =
                open_device_sink(None, &cmd_tx, &error_flag).expect("no audio output device");
            let shared_mixer = Arc::new(Mutex::new(device_sink.mixer().clone()));
            mixer_tx.send(shared_mixer.clone()).ok();

            loop {
                match cmd_rx.recv() {
                    Ok(AudioThreadCmd::SwitchDevice { name, reply }) => {
                        // Drop old sink first
                        drop(device_sink);

                        match open_device_sink(name.as_deref(), &cmd_tx, &error_flag) {
                            Ok(new_sink) => {
                                let mixer = new_sink.mixer().clone();
                                *shared_mixer.lock().unwrap() = mixer.clone();
                                device_sink = new_sink;
                                reply.send(Ok(mixer)).ok();
                            }
                            Err(e) => {
                                // Fallback to default
                                device_sink =
                                    open_device_sink(None, &cmd_tx, &error_flag).expect("no audio output device");
                                *shared_mixer.lock().unwrap() = device_sink.mixer().clone();
                                reply.send(Err(e)).ok();
                            }
                        }
                    }
                    Ok(AudioThreadCmd::Reconnect) => {
                        eprintln!("[audio] device invalidated, reconnecting...");
                        // Small delay to let the OS settle after BT profile switch
                        std::thread::sleep(Duration::from_millis(500));

                        drop(device_sink);
                        match open_device_sink(None, &cmd_tx, &error_flag) {
                            Ok(new_sink) => {
                                *shared_mixer.lock().unwrap() = new_sink.mixer().clone();
                                device_sink = new_sink;
                                reconnected.store(true, Ordering::Relaxed);
                                eprintln!("[audio] reconnected successfully");
                            }
                            Err(e) => {
                                eprintln!("[audio] reconnect failed: {e}, retrying...");
                                std::thread::sleep(Duration::from_secs(1));
                                device_sink =
                                    open_device_sink(None, &cmd_tx, &error_flag).expect("no audio output device");
                                *shared_mixer.lock().unwrap() = device_sink.mixer().clone();
                                reconnected.store(true, Ordering::Relaxed);
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
        mixer: shared_mixer,
        eq_params: Arc::new(RwLock::new(EqParams::default())),
        normalization_enabled: AtomicBool::new(true),
        volume: Mutex::new(0.25), // 50/200
        has_track: AtomicBool::new(false),
        ended_notified: AtomicBool::new(false),
        device_error: device_error_flag,
        device_reconnected: reconnected_flag,
        load_gen: AtomicU64::new(0),
        media_tx: Mutex::new(None),
        audio_tx: cmd_tx,
        source_bytes: Mutex::new(None),
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

            // Check if audio device was reconnected (e.g. BT profile switch)
            if state.device_reconnected.swap(false, Ordering::Relaxed) {
                handle.emit("audio:device-reconnected", ()).ok();
            }

            if !state.has_track.load(Ordering::Relaxed) {
                continue;
            }

            let player = state.player.lock().unwrap();
            if let Some(ref p) = *player {
                if p.empty() {
                    // Suppress track-end during device error (BT profile switch etc.)
                    if !state.device_error.load(Ordering::Relaxed)
                        && !state.ended_notified.swap(true, Ordering::Relaxed)
                    {
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
pub fn audio_load_file(path: String, state: tauri::State<'_, AudioState>) -> Result<AudioLoadResult, String> {
    let bytes =
        std::fs::read(&path).map_err(|e| format!("Failed to read {}: {}", path, e))?;

    // Stop old player BEFORE creating new one to prevent overlap
    {
        let mut player = state.player.lock().unwrap();
        if let Some(old) = player.take() {
            old.stop();
        }
    }

    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let (new_player, duration_secs) = create_player_from_bytes(
        &bytes,
        &mixer,
        vol,
        normalization_enabled,
        state.eq_params.clone(),
    )?;

    *state.player.lock().unwrap() = Some(new_player);
    *state.source_bytes.lock().unwrap() = Some(bytes);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);
    state.device_error.store(false, Ordering::Relaxed);

    Ok(AudioLoadResult { duration_secs })
}

#[derive(serde::Serialize)]
pub struct AudioLoadResult {
    pub duration_secs: Option<f64>,
}

/// Load and play audio from a URL (downloads fully, optionally caches).
#[tauri::command]
pub async fn audio_load_url(
    url: String,
    session_id: Option<String>,
    cache_path: Option<String>,
    state: tauri::State<'_, AudioState>,
) -> Result<AudioLoadResult, String> {
    let gen = state.load_gen.load(Ordering::Relaxed);

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

    let empty_result = AudioLoadResult { duration_secs: None };

    // Stale check after download — another track may have started loading
    if state.load_gen.load(Ordering::Relaxed) != gen {
        return Ok(empty_result);
    }

    // Cache in background
    if let Some(path) = cache_path {
        let data = bytes.clone();
        tokio::spawn(async move {
            tokio::fs::write(&path, &data).await.ok();
        });
    }

    // Stop old player BEFORE creating new one to prevent overlap
    {
        let mut player = state.player.lock().unwrap();
        if let Some(old) = player.take() {
            old.stop();
        }
    }

    // Stale check again after stopping
    if state.load_gen.load(Ordering::Relaxed) != gen {
        return Ok(empty_result);
    }

    // Decode and play
    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let (new_player, duration_secs) = create_player_from_bytes(
        &bytes,
        &mixer,
        vol,
        normalization_enabled,
        state.eq_params.clone(),
    )?;

    *state.player.lock().unwrap() = Some(new_player);
    *state.source_bytes.lock().unwrap() = Some(bytes);
    state.has_track.store(true, Ordering::Relaxed);
    state.ended_notified.store(false, Ordering::Relaxed);
    state.device_error.store(false, Ordering::Relaxed);

    Ok(AudioLoadResult { duration_secs })
}

#[tauri::command]
pub fn audio_play(state: tauri::State<'_, AudioState>) {
    if let Ok(player) = state.player.try_lock() {
        if let Some(ref p) = *player {
            p.play();
        }
    }
}

#[tauri::command]
pub fn audio_pause(state: tauri::State<'_, AudioState>) {
    if let Ok(player) = state.player.try_lock() {
        if let Some(ref p) = *player {
            p.pause();
        }
    }
}

#[tauri::command]
pub fn audio_stop(state: tauri::State<'_, AudioState>) {
    // Use try_lock to avoid blocking IPC if another thread holds the lock (e.g. stuck stop())
    state.has_track.store(false, Ordering::Relaxed);
    state.load_gen.fetch_add(1, Ordering::Relaxed);
    if let Ok(mut player) = state.player.try_lock() {
        if let Some(old) = player.take() {
            old.stop();
        }
    }
    if let Ok(mut bytes) = state.source_bytes.try_lock() {
        *bytes = None;
    }
}

#[tauri::command]
pub fn audio_seek(position: f64, state: tauri::State<'_, AudioState>) -> Result<(), String> {
    let target = Duration::from_secs_f64(position);
    let was_paused = state
        .player
        .lock()
        .unwrap()
        .as_ref()
        .map(|p| p.is_paused())
        .unwrap_or(false);

    // Try normal seek first
    {
        let player = state.player.lock().unwrap();
        if let Some(ref p) = *player {
            if p.try_seek(target).is_ok() {
                return Ok(());
            }
        }
    }

    // Fallback: reload from saved source bytes and seek forward
    let bytes = state.source_bytes.lock().unwrap().clone();
    let Some(bytes) = bytes else {
        return Err("No source to reload for seek".into());
    };

    let mixer = state.mixer.lock().unwrap().clone();
    let vol = *state.volume.lock().unwrap();
    let normalization_enabled = state.normalization_enabled.load(Ordering::Relaxed);
    let (new_player, _) = create_player_from_bytes(
        &bytes,
        &mixer,
        vol,
        normalization_enabled,
        state.eq_params.clone(),
    )?;

    if was_paused {
        new_player.pause();
    }

    if position > 0.0 {
        new_player.try_seek(target).ok();
    }

    let mut player = state.player.lock().unwrap();
    if let Some(old) = player.take() {
        old.stop();
    }
    *player = Some(new_player);
    state.ended_notified.store(false, Ordering::Relaxed);

    if was_paused {
        if let Some(ref p) = *player {
            p.pause();
        }
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
pub fn audio_set_normalization(enabled: bool, state: tauri::State<'_, AudioState>) {
    state.normalization_enabled.store(enabled, Ordering::Relaxed);
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

/* ── Audio Device Management ──────────────────────────────── */

/// Audio sink info from PulseAudio/PipeWire
#[derive(serde::Serialize, Clone)]
pub struct AudioSink {
    pub name: String,        // internal name for pactl
    pub description: String, // human-readable
    pub is_default: bool,
}

#[tauri::command]
pub fn audio_list_devices() -> Vec<AudioSink> {
    #[cfg(target_os = "linux")]
    {
        audio_list_devices_pactl()
    }
    #[cfg(not(target_os = "linux"))]
    {
        audio_list_devices_cpal()
    }
}

/// Linux: pactl returns clean PipeWire/PulseAudio sinks (no ALSA plugin spam)
#[cfg(target_os = "linux")]
fn audio_list_devices_pactl() -> Vec<AudioSink> {
    let output = match std::process::Command::new("pactl")
        .args(["--format=json", "list", "sinks"])
        .output()
    {
        Ok(o) if o.status.success() => o.stdout,
        _ => return Vec::new(),
    };

    let default_sink = std::process::Command::new("pactl")
        .args(["get-default-sink"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();

    let sinks: Vec<serde_json::Value> = match serde_json::from_slice(&output) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    sinks
        .iter()
        .filter_map(|s| {
            let name = s.get("name")?.as_str()?.to_string();
            let description = s.get("description")?.as_str()?.to_string();
            Some(AudioSink {
                is_default: name == default_sink,
                name,
                description,
            })
        })
        .collect()
}

/// Windows/macOS: cpal returns clean device list
#[cfg(not(target_os = "linux"))]
fn audio_list_devices_cpal() -> Vec<AudioSink> {
    use cpal::traits::{DeviceTrait, HostTrait};

    let host = cpal::default_host();
    let default_id = host
        .default_output_device()
        .and_then(|d| d.id().ok())
        .map(|id| id.to_string());

    let devices = match host.output_devices() {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    devices
        .filter_map(|dev| {
            let id = dev.id().ok()?.to_string();
            let description = dev
                .description()
                .ok()
                .map(|d| d.name().to_string())
                .unwrap_or_else(|| id.clone());
            Some(AudioSink {
                is_default: default_id.as_deref() == Some(id.as_str()),
                name: id,
                description,
            })
        })
        .collect()
}

#[tauri::command]
pub fn audio_switch_device(
    device_name: Option<String>,
    state: tauri::State<'_, AudioState>,
) -> Result<(), String> {
    // On Linux, set PipeWire/PulseAudio default sink first, then reopen default cpal device.
    // On other platforms, open the cpal device directly by id.
    #[cfg(target_os = "linux")]
    let switch_name: Option<String> = {
        if let Some(ref name) = device_name {
            std::process::Command::new("pactl")
                .args(["set-default-sink", name])
                .status()
                .map_err(|e| format!("pactl failed: {}", e))?;
        }
        None // always reopen default — pactl already switched it
    };
    #[cfg(not(target_os = "linux"))]
    let switch_name: Option<String> = device_name;

    // Stop current playback
    {
        let mut player = state.player.lock().unwrap();
        if let Some(old) = player.take() {
            old.stop();
        }
        state.has_track.store(false, Ordering::Relaxed);
        state.load_gen.fetch_add(1, Ordering::Relaxed);
    }

    let (reply_tx, reply_rx) = std::sync::mpsc::channel();
    state
        .audio_tx
        .send(AudioThreadCmd::SwitchDevice {
            name: switch_name,
            reply: reply_tx,
        })
        .map_err(|e| e.to_string())?;

    let new_mixer = reply_rx
        .recv()
        .map_err(|e| format!("Device switch failed: {}", e))?
        .map_err(|e| e)?;

    *state.mixer.lock().unwrap() = new_mixer;
    Ok(())
}

/* ── Track Download ───────────────────────────────────────── */

#[tauri::command]
pub async fn save_track_to_path(
    cache_path: String,
    dest_path: String,
) -> Result<String, String> {
    tokio::fs::copy(&cache_path, &dest_path)
        .await
        .map_err(|e| format!("Copy failed: {}", e))?;
    Ok(dest_path)
}
