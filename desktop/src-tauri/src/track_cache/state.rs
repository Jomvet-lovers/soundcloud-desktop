use std::collections::HashMap;
use std::error::Error as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use reqwest::{Client, Url};
use tauri::Emitter;
use tokio::fs::File;
use tokio::io::{AsyncWriteExt, BufWriter};
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, Semaphore};

use crate::shared::constants::STORAGE_BASE_URL;

const MIN_AUDIO_SIZE: u64 = 8192;
const AUDIO_SNIFF_LEN: usize = 16;
const STREAM_WRITE_BUFFER_SIZE: usize = 256 * 1024;
const STORAGE_CONNECT_TIMEOUT_MS: u64 = 800;
const STORAGE_TIMEOUT_MS: u64 = 1200;
const STORAGE_COOLDOWN_SECS: u64 = 60;
const DOWNLOAD_CONNECT_TIMEOUT_MS: u64 = 1500;
const DOWNLOAD_READ_TIMEOUT_SECS: u64 = 20;
const RETRY_DELAYS_MS: [u64; 3] = [200, 600, 1500];
const MAX_PARALLEL_PRELOADS: usize = 20;
const CACHE_METADATA_EXT: &str = ".meta.json";

/// Magic-byte validation for audio files
fn is_valid_audio(prefix: &[u8], total_size: u64) -> bool {
    if total_size < MIN_AUDIO_SIZE {
        return false;
    }
    // ID3 (MP3)
    if prefix.len() >= 3 && prefix[0] == 0x49 && prefix[1] == 0x44 && prefix[2] == 0x33 {
        return true;
    }
    // MPEG Sync (MP3 / ADTS AAC)
    if prefix.len() >= 2 && prefix[0] == 0xff && (prefix[1] & 0xe0) == 0xe0 {
        return true;
    }
    // ftyp (MP4/AAC)
    if prefix.len() >= 8
        && prefix[4] == 0x66
        && prefix[5] == 0x74
        && prefix[6] == 0x79
        && prefix[7] == 0x70
    {
        return true;
    }
    // OggS
    if prefix.len() >= 4
        && prefix[0] == 0x4f
        && prefix[1] == 0x67
        && prefix[2] == 0x67
        && prefix[3] == 0x53
    {
        return true;
    }
    // RIFF/WAV
    if prefix.len() >= 4
        && prefix[0] == 0x52
        && prefix[1] == 0x49
        && prefix[2] == 0x46
        && prefix[3] == 0x46
    {
        return true;
    }
    // fLaC
    if prefix.len() >= 4
        && prefix[0] == 0x66
        && prefix[1] == 0x4c
        && prefix[2] == 0x61
        && prefix[3] == 0x43
    {
        return true;
    }
    false
}

fn urn_to_filename(urn: &str) -> String {
    format!("{}.audio", urn.replace(':', "_"))
}

fn filename_to_urn(filename: &str) -> Option<String> {
    let stripped = filename.strip_suffix(".audio")?;
    Some(stripped.replace('_', ":"))
}

fn is_audio_cache_file(path: &Path) -> bool {
    path.extension().and_then(|ext| ext.to_str()) == Some("audio")
}

fn cache_metadata_path(path: &Path) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), CACHE_METADATA_EXT))
}

fn remove_cache_metadata(path: &Path) {
    std::fs::remove_file(cache_metadata_path(path)).ok();
}

fn truncate_error_text(text: &str, max_chars: usize) -> String {
    let truncated: String = text.chars().take(max_chars).collect();
    if text.chars().count() > max_chars {
        format!("{}...", truncated.trim_end())
    } else {
        truncated
    }
}

fn extract_json_error(value: &serde_json::Value) -> Option<String> {
    if let Some(message) = value.get("message").and_then(|v| v.as_str()) {
        return Some(message.to_string());
    }
    if let Some(error) = value.get("error").and_then(|v| v.as_str()) {
        return Some(error.to_string());
    }
    if let Some(errors) = value.get("errors").and_then(|v| v.as_array()) {
        let parts = errors
            .iter()
            .filter_map(|entry| {
                entry
                    .get("error_message")
                    .and_then(|v| v.as_str())
                    .or_else(|| entry.get("message").and_then(|v| v.as_str()))
                    .or_else(|| entry.get("error").and_then(|v| v.as_str()))
                    .map(str::to_string)
            })
            .collect::<Vec<_>>();
        if !parts.is_empty() {
            return Some(parts.join("; "));
        }
    }
    None
}

fn normalize_error_body(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }

    let compact = if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        extract_json_error(&value).unwrap_or_else(|| value.to_string())
    } else {
        trimmed.to_string()
    };

    let single_line = compact.split_whitespace().collect::<Vec<_>>().join(" ");
    if single_line.is_empty() {
        None
    } else {
        Some(truncate_error_text(&single_line, 220))
    }
}

fn format_reqwest_error(err: reqwest::Error) -> String {
    let mut details = Vec::new();
    if err.is_timeout() {
        details.push("timeout".to_string());
    } else if err.is_connect() {
        details.push("connect".to_string());
    } else if err.is_redirect() {
        details.push("redirect".to_string());
    } else if err.is_body() {
        details.push("body".to_string());
    } else if err.is_decode() {
        details.push("decode".to_string());
    } else if err.is_request() {
        details.push("request".to_string());
    }

    if let Some(status) = err.status() {
        details.push(format!("HTTP {status}"));
    }

    let mut causes = Vec::new();
    let mut source = err.source();
    while let Some(next) = source {
        let text = next.to_string();
        if !text.is_empty() && !causes.iter().any(|existing| existing == &text) {
            causes.push(text);
        }
        source = next.source();
    }

    let mut message = err.without_url().to_string();
    if !details.is_empty() {
        message.push_str(&format!(" [{}]", details.join(", ")));
    }
    if !causes.is_empty() {
        message.push_str(&format!(": {}", causes.join(": ")));
    }
    message
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackQuality {
    Hq,
    Sq,
}

impl PlaybackQuality {
    fn label(self) -> &'static str {
        match self {
            Self::Hq => "hq",
            Self::Sq => "sq",
        }
    }
}

/// Tracks active downloads so duplicate requests coalesce.
struct ActiveDownload {
    notify: Arc<Notify>,
    result: Arc<Mutex<Option<Result<PathBuf, String>>>>,
}

#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DownloadSource {
    Storage,
    Api,
}

impl DownloadSource {
    fn label(self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Api => "api",
        }
    }
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct TrackCacheMetadata {
    quality: PlaybackQuality,
    #[serde(default)]
    source: Option<DownloadSource>,
}

#[derive(Clone, serde::Serialize)]
pub struct TrackCacheEntry {
    pub path: String,
    pub quality: Option<String>,
    pub source: Option<String>,
}

impl TrackCacheEntry {
    fn from_path_and_meta(path: &Path, meta: Option<TrackCacheMetadata>) -> Self {
        Self {
            path: path.to_string_lossy().into_owned(),
            quality: meta.as_ref().map(|m| m.quality.label().to_string()),
            source: meta.and_then(|m| m.source.map(|s| s.label().to_string())),
        }
    }
}

enum DownloadError {
    Fatal(String),
    Retryable(String),
}

struct DownloadResult {
    path: PathBuf,
}

/// Circuit breaker: skip storage for STORAGE_COOLDOWN_SECS after a failure.
/// Stores epoch secs of last failure; 0 = never failed.
static STORAGE_FAILED_AT: AtomicU64 = AtomicU64::new(0);

fn storage_available() -> bool {
    let failed_at = STORAGE_FAILED_AT.load(Ordering::Relaxed);
    if failed_at == 0 {
        return true;
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    now.saturating_sub(failed_at) >= STORAGE_COOLDOWN_SECS
}

fn mark_storage_failed() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    STORAGE_FAILED_AT.store(now, Ordering::Relaxed);
}

fn mark_storage_ok() {
    STORAGE_FAILED_AT.store(0, Ordering::Relaxed);
}

fn build_storage_url(urn: &str, prefer_hq: bool) -> String {
    let quality = if prefer_hq { "hq" } else { "sq" };
    format!(
        "{}/{}/{}.ogg",
        STORAGE_BASE_URL,
        quality,
        urn.replace(':', "_")
    )
}

#[derive(Clone)]
pub struct TrackCacheState {
    pub audio_dir: PathBuf,
    pub client: Client,
    pub storage_client: Client,
    pub app_handle: Option<tauri::AppHandle>,
    active: Arc<Mutex<HashMap<String, ActiveDownload>>>,
    preload_limiter: Arc<Semaphore>,
}

pub fn init(audio_dir: PathBuf) -> TrackCacheState {
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .tcp_nodelay(true)
        .pool_max_idle_per_host(16)
        .connect_timeout(Duration::from_millis(DOWNLOAD_CONNECT_TIMEOUT_MS))
        .read_timeout(Duration::from_secs(DOWNLOAD_READ_TIMEOUT_SECS))
        .build()
        .expect("failed to build reqwest client");

    let storage_client = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .tcp_nodelay(true)
        .pool_max_idle_per_host(4)
        .connect_timeout(Duration::from_millis(STORAGE_CONNECT_TIMEOUT_MS))
        .timeout(Duration::from_millis(STORAGE_TIMEOUT_MS))
        .build()
        .expect("failed to build storage client");

    TrackCacheState {
        audio_dir,
        client,
        storage_client,
        app_handle: None,
        active: Arc::new(Mutex::new(HashMap::new())),
        preload_limiter: Arc::new(Semaphore::new(MAX_PARALLEL_PRELOADS)),
    }
}

fn quality_from_url(url: &str) -> PlaybackQuality {
    Url::parse(url)
        .ok()
        .map(|parsed| {
            parsed
                .query_pairs()
                .any(|(key, value)| key == "hq" && value == "true")
        })
        .unwrap_or(false)
        .then_some(PlaybackQuality::Hq)
        .unwrap_or(PlaybackQuality::Sq)
}

fn temp_file_path(audio_dir: &Path, urn: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    audio_dir.join(format!("{}.{}.part", urn_to_filename(urn), nonce))
}

async fn cleanup_temp_file(path: &Path) {
    tokio::fs::remove_file(path).await.ok();
}

fn read_cache_metadata(path: &Path) -> Option<TrackCacheMetadata> {
    let raw = std::fs::read_to_string(cache_metadata_path(path)).ok()?;
    serde_json::from_str(&raw).ok()
}

async fn write_cache_metadata(path: &Path, meta: &TrackCacheMetadata) {
    let raw = match serde_json::to_vec(meta) {
        Ok(raw) => raw,
        Err(_) => return,
    };

    let final_path = cache_metadata_path(path);
    let temp_path = PathBuf::from(format!("{}.tmp", final_path.display()));
    if tokio::fs::write(&temp_path, raw).await.is_err() {
        tokio::fs::remove_file(&temp_path).await.ok();
        return;
    }

    if tokio::fs::rename(&temp_path, &final_path).await.is_err() {
        tokio::fs::remove_file(&temp_path).await.ok();
    }
}

async fn write_response_to_cache(
    audio_dir: &Path,
    urn: &str,
    response: reqwest::Response,
    quality: PlaybackQuality,
    source: DownloadSource,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<DownloadResult, DownloadError> {
    let final_path = audio_dir.join(urn_to_filename(urn));
    let temp_path = temp_file_path(audio_dir, urn);
    let file = File::create(&temp_path)
        .await
        .map_err(|err| DownloadError::Fatal(format!("Cache create failed: {err}")))?;
    let mut writer = BufWriter::with_capacity(STREAM_WRITE_BUFFER_SIZE, file);
    let content_length = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();
    let mut total_size = 0u64;
    let mut sniff = Vec::with_capacity(AUDIO_SNIFF_LEN);

    while let Some(chunk) = stream.next().await {
        let chunk = match chunk {
            Ok(chunk) => chunk,
            Err(err) => {
                cleanup_temp_file(&temp_path).await;
                return Err(DownloadError::Retryable(format!("body read: {err}")));
            }
        };

        total_size += chunk.len() as u64;
        if sniff.len() < AUDIO_SNIFF_LEN {
            let copy_len = (AUDIO_SNIFF_LEN - sniff.len()).min(chunk.len());
            sniff.extend_from_slice(&chunk[..copy_len]);
        }

        if let Err(err) = writer.write_all(&chunk).await {
            cleanup_temp_file(&temp_path).await;
            return Err(DownloadError::Fatal(format!("Cache write failed: {err}")));
        }

        if let Some(app) = app_handle {
            if content_length > 0 {
                let _ = app.emit(
                    "track:download-progress",
                    serde_json::json!({
                        "urn": urn,
                        "downloaded": total_size,
                        "total": content_length,
                        "progress": total_size as f64 / content_length as f64,
                        "source": source.label(),
                    }),
                );
            }
        }
    }

    if let Err(err) = writer.flush().await {
        cleanup_temp_file(&temp_path).await;
        return Err(DownloadError::Fatal(format!("Cache flush failed: {err}")));
    }
    drop(writer);

    if !is_valid_audio(&sniff, total_size) {
        cleanup_temp_file(&temp_path).await;
        return Err(DownloadError::Fatal("Invalid audio data".into()));
    }

    let cache_meta = TrackCacheMetadata { quality, source: Some(source) };

    if let Ok(meta) = tokio::fs::metadata(&final_path).await {
        if meta.len() >= MIN_AUDIO_SIZE {
            cleanup_temp_file(&temp_path).await;
            return Ok(DownloadResult { path: final_path });
        }
    }

    match tokio::fs::rename(&temp_path, &final_path).await {
        Ok(()) => {
            write_cache_metadata(&final_path, &cache_meta).await;
            Ok(DownloadResult { path: final_path })
        }
        Err(first_err) => {
            if tokio::fs::metadata(&final_path)
                .await
                .map(|meta| meta.len() >= MIN_AUDIO_SIZE)
                .unwrap_or(false)
            {
                cleanup_temp_file(&temp_path).await;
                return Ok(DownloadResult { path: final_path });
            }

            tokio::fs::remove_file(&final_path).await.ok();
            match tokio::fs::rename(&temp_path, &final_path).await {
                Ok(()) => {
                    write_cache_metadata(&final_path, &cache_meta).await;
                    Ok(DownloadResult { path: final_path })
                }
                Err(second_err) => {
                    cleanup_temp_file(&temp_path).await;
                    Err(DownloadError::Fatal(format!(
                        "Cache rename failed: {first_err}; {second_err}"
                    )))
                }
            }
        }
    }
}

/// Download a track from an API URL to cache.
async fn download_api(
    client: &Client,
    audio_dir: &Path,
    urn: &str,
    url: &str,
    session_id: Option<&str>,
    app_handle: Option<&tauri::AppHandle>,
) -> Result<DownloadResult, DownloadError> {
    let mut req = client.get(url);
    if let Some(sid) = session_id {
        req = req.header("x-session-id", sid);
    }

    let response = req.send().await.map_err(|err| {
        DownloadError::Retryable(format!("request: {}", format_reqwest_error(err)))
    })?;
    let status = response.status();

    if status.is_success() {
        let quality = quality_from_url(url);
        return write_response_to_cache(
            audio_dir, urn, response, quality, DownloadSource::Api, app_handle,
        )
        .await;
    }

    let body = match response.text().await {
        Ok(body) => normalize_error_body(&body),
        Err(err) => Some(format!(
            "failed to read response body: {}",
            format_reqwest_error(err)
        )),
    };
    let message = if let Some(body) = body {
        format!("HTTP {}: {}", status, body)
    } else {
        format!("HTTP {}", status)
    };
    Err(DownloadError::Retryable(message))
}

impl TrackCacheState {
    pub fn try_acquire_preload_slot(&self) -> Option<OwnedSemaphorePermit> {
        self.preload_limiter.clone().try_acquire_owned().ok()
    }

    fn file_path(&self, urn: &str) -> PathBuf {
        self.audio_dir.join(urn_to_filename(urn))
    }

    pub fn is_cached(&self, urn: &str) -> bool {
        let path = self.file_path(urn);
        match std::fs::metadata(&path) {
            Ok(meta) => meta.len() >= MIN_AUDIO_SIZE,
            Err(_) => false,
        }
    }

    pub fn get_cache_path(&self, urn: &str) -> Option<String> {
        if self.is_cached(urn) {
            Some(self.file_path(urn).to_string_lossy().into_owned())
        } else {
            None
        }
    }

    pub fn get_cache_entry(&self, urn: &str) -> Option<TrackCacheEntry> {
        let path = self.file_path(urn);
        if !self.is_cached(urn) {
            return None;
        }
        Some(TrackCacheEntry::from_path_and_meta(
            &path,
            read_cache_metadata(&path),
        ))
    }

    /// Download track, save to cache. Coalesces concurrent requests for the same URN.
    /// Tries each URL in order with retries, falling back to the next on failure.
    pub async fn ensure_cached(
        &self,
        urn: &str,
        urls: &[String],
        session_id: Option<&str>,
    ) -> Result<TrackCacheEntry, String> {
        if let Some(entry) = self.get_cache_entry(urn) {
            println!("[TrackCache] hit: {urn}");
            return Ok(entry);
        }

        // Coalesce concurrent requests for the same URN
        let mut active = self.active.lock().await;
        if let Some(existing) = active.get(urn) {
            println!("[TrackCache] coalescing request for {urn}");
            let notify = existing.notify.clone();
            let result_slot = existing.result.clone();
            drop(active);
            notify.notified().await;
            let res = result_slot.lock().await;
            return match res.as_ref() {
                Some(Ok(path)) => Ok(TrackCacheEntry::from_path_and_meta(
                    path,
                    read_cache_metadata(path),
                )),
                Some(Err(e)) => Err(e.clone()),
                None => Err("download completed without result".into()),
            };
        }

        let notify = Arc::new(Notify::new());
        let result_slot: Arc<Mutex<Option<Result<PathBuf, String>>>> = Arc::new(Mutex::new(None));
        active.insert(
            urn.to_string(),
            ActiveDownload {
                notify: notify.clone(),
                result: result_slot.clone(),
            },
        );
        drop(active);

        let download_result = self.download_with_fallback(urn, urls, session_id).await;

        {
            let mut slot = result_slot.lock().await;
            *slot = Some(download_result.clone());
        }
        notify.notify_waiters();
        self.active.lock().await.remove(urn);

        download_result
            .map(|path| TrackCacheEntry::from_path_and_meta(&path, read_cache_metadata(&path)))
    }

    /// Try storage once, then each API URL in order with retries.
    async fn download_with_fallback(
        &self,
        urn: &str,
        urls: &[String],
        session_id: Option<&str>,
    ) -> Result<PathBuf, String> {
        let start = std::time::Instant::now();
        let mut last_err = String::from("no stream URLs provided");

        // 1. Try storage once (quality from first URL)
        if storage_available() {
            let prefer_hq = urls
                .first()
                .map(|u| matches!(quality_from_url(u), PlaybackQuality::Hq))
                .unwrap_or(false);
            let storage_url = build_storage_url(urn, prefer_hq);

            match self.storage_client.get(&storage_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    mark_storage_ok();
                    let quality = if prefer_hq {
                        PlaybackQuality::Hq
                    } else {
                        PlaybackQuality::Sq
                    };
                    println!("[TrackCache] {urn} → storage");
                    match write_response_to_cache(
                        &self.audio_dir,
                        urn,
                        resp,
                        quality,
                        DownloadSource::Storage,
                        self.app_handle.as_ref(),
                    )
                    .await
                    {
                        Ok(result) => {
                            let kb = std::fs::metadata(&result.path)
                                .map(|m| m.len() / 1024)
                                .unwrap_or(0);
                            let ms = start.elapsed().as_millis();
                            println!("[TrackCache] downloaded {urn} via storage — {kb} KB in {ms}ms");
                            return Ok(result.path);
                        }
                        Err(DownloadError::Fatal(e)) => {
                            eprintln!("[TrackCache] storage write failed for {urn}: {e}");
                        }
                        Err(DownloadError::Retryable(e)) => {
                            eprintln!("[TrackCache] storage download failed for {urn}: {e}");
                        }
                    }
                }
                Ok(resp) if resp.status().as_u16() == 404 || resp.status().as_u16() == 410 => {}
                Ok(resp) => {
                    eprintln!("[TrackCache] storage HTTP {} for {urn}", resp.status());
                    mark_storage_failed();
                }
                Err(err) => {
                    eprintln!("[TrackCache] storage failed for {urn}: {err}");
                    mark_storage_failed();
                }
            }
        }

        // 2. Try each API URL in order
        for (i, url) in urls.iter().enumerate() {
            println!("[TrackCache] trying URL #{} for {urn}", i + 1);

            match self.download_api_with_retries(urn, url, session_id).await {
                Ok(path) => {
                    let kb = std::fs::metadata(&path)
                        .map(|meta| meta.len() / 1024)
                        .unwrap_or(0);
                    let ms = start.elapsed().as_millis();
                    println!("[TrackCache] downloaded {urn} via api — {kb} KB in {ms}ms");
                    return Ok(path);
                }
                Err(err) => {
                    if i + 1 < urls.len() {
                        eprintln!("[TrackCache] {urn} URL #{} failed, trying next: {err}", i + 1);
                    }
                    last_err = err;
                }
            }
        }

        eprintln!("[TrackCache] gave up on {urn}: {last_err}");
        Err(last_err)
    }

    /// Download from a single URL with retries for retryable errors.
    async fn download_api_with_retries(
        &self,
        urn: &str,
        url: &str,
        session_id: Option<&str>,
    ) -> Result<PathBuf, String> {
        let mut last_err = String::new();

        for attempt in 0..=RETRY_DELAYS_MS.len() {
            if attempt > 0 {
                eprintln!("[TrackCache] retry #{attempt} for {urn}: {last_err}");
                tokio::time::sleep(Duration::from_millis(RETRY_DELAYS_MS[attempt - 1])).await;
            }

            match download_api(
                &self.client,
                &self.audio_dir,
                urn,
                url,
                session_id,
                self.app_handle.as_ref(),
            )
            .await
            {
                Ok(result) => return Ok(result.path),
                Err(DownloadError::Fatal(err)) => return Err(err),
                Err(DownloadError::Retryable(err)) => {
                    last_err = err;
                }
            }
        }

        Err(last_err)
    }

    pub fn cache_size(&self) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = std::fs::read_dir(&self.audio_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() && is_audio_cache_file(&entry.path()) {
                        total += meta.len();
                    }
                }
            }
        }
        total
    }

    pub fn clear_cache(&self) {
        if let Ok(entries) = std::fs::read_dir(&self.audio_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if entry.metadata().map(|m| m.is_file()).unwrap_or(false)
                    && is_audio_cache_file(&path)
                {
                    std::fs::remove_file(&path).ok();
                    remove_cache_metadata(&path);
                }
            }
        }
    }

    pub fn list_cached_urns(&self) -> Vec<String> {
        let mut urns = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.audio_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if let Some(urn) = filename_to_urn(&name) {
                    let meta = entry.metadata();
                    if meta.map(|m| m.len() >= MIN_AUDIO_SIZE).unwrap_or(false) {
                        urns.push(urn);
                    } else {
                        let path = entry.path();
                        std::fs::remove_file(&path).ok();
                        remove_cache_metadata(&path);
                    }
                }
            }
        }
        urns
    }

    pub fn enforce_limit(&self, limit_mb: u64) {
        if limit_mb == 0 {
            return;
        }
        let limit_bytes = limit_mb * 1024 * 1024;

        let mut files: Vec<(PathBuf, u64, std::time::SystemTime)> = Vec::new();
        let mut total = 0u64;

        if let Ok(entries) = std::fs::read_dir(&self.audio_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !is_audio_cache_file(&path) {
                    continue;
                }
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let size = meta.len();
                        let accessed = meta
                            .accessed()
                            .or_else(|_| meta.modified())
                            .unwrap_or(std::time::UNIX_EPOCH);
                        total += size;
                        files.push((path, size, accessed));
                    }
                }
            }
        }

        if total <= limit_bytes {
            return;
        }

        let before = total;
        files.sort_by(|a, b| a.2.cmp(&b.2));

        let mut removed = 0u32;
        for (path, size, _) in files {
            if total <= limit_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                remove_cache_metadata(&path);
                total -= size;
                removed += 1;
            }
        }
        println!(
            "[TrackCache] evicted {removed} files, freed {} MB",
            (before - total) / (1024 * 1024)
        );
    }
}
