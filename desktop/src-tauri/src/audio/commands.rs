use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::rt::AppHandle;
use tauri::{Emitter, Manager, State};

use crate::app::diagnostics::log_native;
use crate::audio::device;
use crate::audio::engine;
use crate::audio::state::AudioState;
use crate::audio::stream_buffer::{StreamBuffer, early_start_container};
use crate::audio::timing;
use crate::audio::types::{AudioLoadResult, AudioSink};
use crate::track_cache::EnsureCachedRequest;
use crate::track_cache::state::{CacheRequest, TrackCacheEntry, TrackCacheState};

/// Bytes buffered before the decoder is built. A fraction of the file is enough
/// of a head start for a progressive download; HLS reports no total (segments are
/// mirrored whole), so there the floor is roughly two segments.
const STREAM_PREBUFFER_MAX_BYTES: u64 = 1_000_000;
const STREAM_PREBUFFER_MIN_BYTES: u64 = 256 * 1024;
/// Box scan window for the mp4 `moov` check (a header that does not fit falls
/// back to the full-wait path rather than guessing).
const STREAM_SNIFF_BYTES: usize = 512 * 1024;
const STREAM_POLL_MS: u64 = 20;
/// Below this the mirror never held a usable payload (e.g. a coalesced winner
/// wrote the file), so the finished bytes must not replace the source.
const MIN_MIRROR_BYTES: usize = 8192;

fn prebuffer_target(total: Option<u64>) -> u64 {
    match total {
        Some(total) => (total / 8).clamp(
            STREAM_PREBUFFER_MIN_BYTES.min(total),
            STREAM_PREBUFFER_MAX_BYTES,
        ),
        None => STREAM_PREBUFFER_MIN_BYTES,
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamLoadResult {
    pub duration_secs: Option<f64>,
    /// Playback started on the growing buffer. The cache download keeps running
    /// and `audio:stream-complete` marks the rebuild onto the finished bytes.
    pub streaming: bool,
    pub quality: Option<String>,
    pub source: Option<String>,
}

fn stream_active(app: &AppHandle, buffer: &Arc<StreamBuffer>) -> bool {
    let audio = app.state::<AudioState>();
    engine::is_active_stream(audio.inner(), buffer)
}

#[tauri::command]
pub async fn audio_load_file(
    path: String,
    cache_key: Option<String>,
    start_paused: bool,
    app: AppHandle,
    state: State<'_, AudioState>,
) -> Result<AudioLoadResult, String> {
    let normalization_cache_dir = app
        .path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("audio-normalization"));
    engine::load_file(
        path,
        normalization_cache_dir,
        cache_key,
        start_paused,
        state,
    )
    .await
}

#[tauri::command]
pub async fn audio_load_url(
    url: String,
    session_id: Option<String>,
    cache_path: Option<String>,
    cache_key: Option<String>,
    start_paused: bool,
    app: AppHandle,
    state: State<'_, AudioState>,
) -> Result<AudioLoadResult, String> {
    let normalization_cache_dir = app
        .path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("audio-normalization"));
    engine::load_url(
        url,
        session_id,
        cache_path,
        normalization_cache_dir,
        cache_key,
        start_paused,
        state,
    )
    .await
}

/// Cache-miss load with streaming-first playback.
///
/// The download runs through the normal cache pipeline (same candidates, same
/// order, same quality choice); its bytes are teed into a `StreamBuffer` so the
/// decoder can start after a small prebuffer. Containers that cannot decode from
/// a prefix, and any failed early start, wait for the finished file and load it
/// exactly like `audio_load_file` does.
#[tauri::command]
pub async fn audio_load_stream(
    request: EnsureCachedRequest,
    start_paused: bool,
    app: AppHandle,
    cache: State<'_, TrackCacheState>,
    state: State<'_, AudioState>,
) -> Result<AudioStreamLoadResult, String> {
    let fallback_urls = match (&request.urls, &request.url) {
        (Some(urls), _) if !urls.is_empty() => urls.clone(),
        (_, Some(url)) => vec![url.clone()],
        _ => return Err("no stream URL provided".to_string()),
    };
    let normalization_cache_dir = app
        .path()
        .app_cache_dir()
        .ok()
        .map(|dir| dir.join("audio-normalization"));

    let stream = StreamBuffer::new();
    // Installs the session before the download starts, so a stop or track
    // switch at any point cancels it instead of letting it install a player.
    engine::begin_stream(&state, &stream);

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let session = StreamSession {
        cache: cache.inner().clone(),
        app: app.clone(),
        stream: stream.clone(),
        urn: request.urn.clone(),
        urls: fallback_urls,
        download_urls: request.download_urls.clone().unwrap_or_default(),
        storage_urls: request.storage_urls.clone().unwrap_or_default(),
        session_id: request.session_id.clone(),
        hq: request.hq,
        duration_ms: request.duration_ms,
        start_paused,
        normalization_cache_dir,
        normalization_cache_key: Some(request.urn.clone()),
    };
    tokio::spawn(async move {
        if let Err(err) = session.run(ready_tx).await {
            eprintln!("[Audio] stream session failed: {err}");
        }
    });

    ready_rx
        .await
        .map_err(|_| "stream session dropped".to_string())?
}

struct StreamSession {
    cache: TrackCacheState,
    app: AppHandle,
    stream: Arc<StreamBuffer>,
    urn: String,
    urls: Vec<String>,
    download_urls: Vec<String>,
    storage_urls: Vec<String>,
    session_id: Option<String>,
    hq: bool,
    duration_ms: Option<u64>,
    start_paused: bool,
    normalization_cache_dir: Option<PathBuf>,
    normalization_cache_key: Option<String>,
}

impl StreamSession {
    async fn run(
        self,
        ready: tokio::sync::oneshot::Sender<Result<AudioStreamLoadResult, String>>,
    ) -> Result<(), String> {
        let t0 = std::time::Instant::now();
        let (done_tx, mut done_rx) = tokio::sync::oneshot::channel();
        tokio::spawn({
            let cache = self.cache.clone();
            let stream = self.stream.clone();
            let (urn, urls, download_urls) = (
                self.urn.clone(),
                self.urls.clone(),
                self.download_urls.clone(),
            );
            let (storage_urls, session_id) = (self.storage_urls.clone(), self.session_id.clone());
            let (hq, duration_ms) = (self.hq, self.duration_ms);
            async move {
                let result = cache
                    .ensure_cached(CacheRequest {
                        urn: &urn,
                        urls: &urls,
                        download_urls: &download_urls,
                        storage_urls: &storage_urls,
                        session_id: session_id.as_deref(),
                        hq,
                        liked: false,
                        expected_duration_ms: duration_ms,
                        stream_sink: Some(stream.clone()),
                    })
                    .await;
                if result.is_err() {
                    // Unblock a decoder parked on a read: no more bytes are coming.
                    stream.finish();
                }
                let _ = done_tx.send(result);
            }
        });

        // Wait for the prebuffer, bailing out early if the download finishes or
        // the session is cancelled first.
        let mut cancelled = false;
        let mut early: Option<Result<TrackCacheEntry, String>> = None;
        loop {
            if !stream_active(&self.app, &self.stream) {
                cancelled = true;
                break;
            }
            let snapshot = self.stream.state();
            if snapshot.bytes >= prebuffer_target(snapshot.total) || snapshot.finished {
                break;
            }
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_millis(STREAM_POLL_MS)) => {}
                result = &mut done_rx => {
                    early = Some(result.unwrap_or_else(|_| {
                        Err("download task dropped".to_string())
                    }));
                    break;
                }
            }
        }

        if cancelled {
            self.stream.detach();
            let _ = ready.send(Err("stream cancelled".to_string()));
            // Let the download finish into the cache; nothing will play it.
            let _ = done_rx.await;
            return Ok(());
        }

        // A download that already finished makes early playback pointless: load
        // the file instead of rebuilding a player from the same bytes a moment
        // later. Also catches a failed download before it starts any audio.
        if early.is_none()
            && let Ok(result) = done_rx.try_recv()
        {
            early = Some(result);
        }

        let container = early_start_container(&self.stream.head(STREAM_SNIFF_BYTES));
        let early_start = early.is_none() && container.is_some();
        let started = if early_start {
            let audio = self.app.state::<AudioState>();
            match engine::load_stream(
                self.stream.clone(),
                self.normalization_cache_dir.clone(),
                self.normalization_cache_key.clone(),
                self.start_paused,
                audio.inner(),
            )
            .await
            {
                Ok(load) => {
                    log_native(
                        &self.app,
                        "INFO",
                        format!(
                            "[Audio] {} early start ({}): {} ms to first audio, {} KB prebuffered",
                            self.urn,
                            container.unwrap_or("?"),
                            t0.elapsed().as_millis(),
                            self.stream.state().bytes / 1024,
                        ),
                    );
                    Some(AudioStreamLoadResult {
                        duration_secs: load.duration_secs,
                        streaming: true,
                        quality: self.stream.quality().map(str::to_string),
                        source: self.stream.source().map(str::to_string),
                    })
                }
                Err(err) => {
                    if !stream_active(&self.app, &self.stream) {
                        cancelled = true;
                        None
                    } else {
                        eprintln!("[Audio] early start failed for {}: {err}", self.urn);
                        self.stream.detach();
                        None
                    }
                }
            }
        } else {
            self.stream.detach();
            None
        };

        if cancelled {
            let _ = ready.send(Err("stream cancelled".to_string()));
            let _ = done_rx.await;
            return Ok(());
        }

        if let Some(result) = started {
            let _ = ready.send(Ok(result));
            self.finish_streaming(done_rx).await;
            return Ok(());
        }

        // Full-wait path: identical to the pre-streaming behaviour (the download
        // is already running, so only the load is left).
        let reason = if early.is_some() {
            "download finished before prebuffer"
        } else if container.is_none() {
            "container needs the full file"
        } else {
            "early player build failed"
        };
        let entry = match early {
            Some(result) => result,
            None => done_rx
                .await
                .unwrap_or_else(|_| Err("download task dropped".to_string())),
        };
        log_native(
            &self.app,
            "INFO",
            format!(
                "[Audio] {} full-wait load: {} ms ({reason})",
                self.urn,
                t0.elapsed().as_millis(),
            ),
        );
        let result = self.load_finished(entry).await;
        let _ = ready.send(result);
        Ok(())
    }

    /// The command has answered and audio is playing from the mirror: adopt the
    /// finished file for the rest of the track.
    async fn finish_streaming(
        self,
        done_rx: tokio::sync::oneshot::Receiver<Result<TrackCacheEntry, String>>,
    ) {
        let entry = done_rx
            .await
            .unwrap_or_else(|_| Err("download task dropped".to_string()));
        self.stream.finish();

        if let Err(err) = entry {
            self.stream.cancel();
            let _ = self.app.emit(
                "audio:stream-failed",
                serde_json::json!({ "urn": self.urn, "error": err }),
            );
            return;
        }

        let bytes = self.stream.copy_bytes();
        if bytes.len() >= MIN_MIRROR_BYTES {
            let rebuild_t0 = std::time::Instant::now();
            let stream = self.stream.clone();
            let app = self.app.clone();
            let dir = self.normalization_cache_dir.clone();
            let key = self.normalization_cache_key.clone();
            let rebuilt = tokio::task::spawn_blocking(move || {
                let audio = app.state::<AudioState>();
                engine::finalize_stream(audio.inner(), &stream, bytes, dir, key)
            })
            .await
            .unwrap_or(false);
            if rebuilt {
                log_native(
                    &self.app,
                    "INFO",
                    format!(
                        "[Audio] {} rebuild onto finished bytes: {} ms",
                        self.urn,
                        rebuild_t0.elapsed().as_millis(),
                    ),
                );
            } else {
                eprintln!(
                    "[Audio] stream finalize skipped for {} (session replaced)",
                    self.urn
                );
            }
        }

        // Emitted even when the rebuild was skipped: it only means "the download
        // behind this track is done", which is what clears the progress readout.
        let _ = self.app.emit(
            "audio:stream-complete",
            serde_json::json!({ "urn": self.urn }),
        );
    }

    /// Load the finished cache entry (or the re-resolved current path when the
    /// background transcode swapped it between resolve and read).
    async fn load_finished(
        &self,
        entry: Result<TrackCacheEntry, String>,
    ) -> Result<AudioStreamLoadResult, String> {
        let entry = entry?;
        let audio = self.app.state::<AudioState>();
        let state = audio.inner();
        let load = match engine::load_file_inner(
            state,
            entry.path.clone(),
            self.normalization_cache_dir.clone(),
            self.normalization_cache_key.clone(),
            self.start_paused,
        )
        .await
        {
            Ok(load) => load,
            Err(err) => match self.cache.get_cache_path(&self.urn) {
                Some(path) if path != entry.path => {
                    engine::load_file_inner(
                        state,
                        path,
                        self.normalization_cache_dir.clone(),
                        self.normalization_cache_key.clone(),
                        self.start_paused,
                    )
                    .await?
                }
                _ => return Err(err),
            },
        };
        Ok(AudioStreamLoadResult {
            duration_secs: load.duration_secs,
            streaming: false,
            quality: entry.quality,
            source: entry.source,
        })
    }
}

#[tauri::command]
pub fn audio_play(state: State<'_, AudioState>) {
    engine::play(state);
}

#[tauri::command]
pub fn audio_pause(state: State<'_, AudioState>) {
    engine::pause(state);
}

#[tauri::command]
pub fn audio_stop(state: State<'_, AudioState>) {
    engine::stop(state);
}

#[tauri::command]
pub async fn audio_seek(position: f64, app: AppHandle) -> Result<(), String> {
    // Off the main thread: a seek on a streaming-first-play session can wait for
    // the target bytes to arrive (or for the download to end) before the decoder
    // applies it.
    tokio::task::spawn_blocking(move || {
        let state = app.state::<AudioState>();
        engine::seek(position, state)
    })
    .await
    .map_err(|e| format!("audio seek task failed: {e}"))?
}

#[tauri::command]
pub fn audio_set_volume(volume: f64, state: State<'_, AudioState>) {
    engine::set_volume(volume, state);
}

#[tauri::command]
pub fn audio_set_playback_rate(rate: f64, state: State<'_, AudioState>) {
    engine::set_playback_rate(rate, state);
}

#[tauri::command]
pub fn audio_set_ab_loop(a: Option<f64>, b: Option<f64>, state: State<'_, AudioState>) {
    engine::set_ab_loop(a, b, state);
}

#[tauri::command]
pub fn audio_get_position(state: State<'_, AudioState>) -> f64 {
    engine::get_position(state)
}

#[tauri::command]
pub fn audio_set_eq(enabled: bool, gains: Vec<f64>, state: State<'_, AudioState>) {
    engine::set_eq(enabled, gains, state);
}

#[tauri::command]
pub fn audio_set_normalization(enabled: bool, state: State<'_, AudioState>) {
    engine::set_normalization(enabled, state);
}

#[tauri::command]
pub fn audio_is_playing(state: State<'_, AudioState>) -> bool {
    engine::is_playing(state)
}

#[tauri::command]
pub fn audio_set_metadata(
    title: String,
    artist: String,
    cover_url: Option<String>,
    duration_secs: f64,
    state: State<'_, AudioState>,
) {
    engine::set_metadata(title, artist, cover_url, duration_secs, state);
}

#[tauri::command]
pub fn audio_set_playback_state(playing: bool, state: State<'_, AudioState>) {
    engine::set_playback_state(playing, state);
}

#[tauri::command]
pub fn audio_set_media_position(position: f64, state: State<'_, AudioState>) {
    engine::set_media_position(position, state);
}

#[tauri::command]
pub fn audio_list_devices() -> Vec<AudioSink> {
    device::list_devices()
}

#[tauri::command]
pub fn audio_switch_device(
    device_name: Option<String>,
    state: State<'_, AudioState>,
) -> Result<(), String> {
    device::switch_device(state, device_name)
}

#[tauri::command]
pub fn audio_set_follow_default_output(follow: bool, state: State<'_, AudioState>) {
    device::set_follow_default_output(state, follow);
}

#[tauri::command]
pub fn audio_get_follow_default_output(state: State<'_, AudioState>) -> bool {
    device::get_follow_default_output(state)
}

#[tauri::command]
pub async fn save_track_to_path(cache_path: String, dest_path: String) -> Result<String, String> {
    engine::save_track_to_path(cache_path, dest_path).await
}

#[tauri::command]
pub fn audio_set_lyrics_timeline(
    lines: Vec<crate::audio::types::LyricsTimingLine>,
    state: State<'_, AudioState>,
) {
    timing::audio_set_lyrics_timeline(lines, state);
}

#[tauri::command]
pub fn audio_clear_lyrics_timeline(state: State<'_, AudioState>) {
    timing::audio_clear_lyrics_timeline(state);
}

#[tauri::command]
pub fn audio_set_comments_timeline(
    comments: Vec<crate::audio::types::FloatingCommentEvent>,
    state: State<'_, AudioState>,
) {
    timing::audio_set_comments_timeline(comments, state);
}

#[tauri::command]
pub fn audio_clear_comments_timeline(state: State<'_, AudioState>) {
    timing::audio_clear_comments_timeline(state);
}

#[tauri::command]
pub async fn audio_preview_play(
    path: String,
    volume: f64,
    r#gen: u64,
    state: State<'_, AudioState>,
) -> Result<(), String> {
    engine::preview_play(path, volume, r#gen, state).await
}

#[tauri::command]
pub fn audio_preview_stop(fade_ms: u64, r#gen: u64, state: State<'_, AudioState>) {
    engine::preview_stop(fade_ms, r#gen, state);
}
