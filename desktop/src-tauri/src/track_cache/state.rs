use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{Mutex, Notify};

const MIN_AUDIO_SIZE: u64 = 8192;

/// Magic-byte validation for audio files
fn is_valid_audio(data: &[u8]) -> bool {
    if data.len() < MIN_AUDIO_SIZE as usize {
        return false;
    }
    // ID3 (MP3)
    if data[0] == 0x49 && data[1] == 0x44 && data[2] == 0x33 {
        return true;
    }
    // MPEG Sync (MP3 / ADTS AAC)
    if data[0] == 0xff && (data[1] & 0xe0) == 0xe0 {
        return true;
    }
    // ftyp (MP4/AAC)
    if data.len() >= 8 && data[4] == 0x66 && data[5] == 0x74 && data[6] == 0x79 && data[7] == 0x70
    {
        return true;
    }
    // OggS
    if data[0] == 0x4f && data[1] == 0x67 && data[2] == 0x67 && data[3] == 0x53 {
        return true;
    }
    // RIFF/WAV
    if data[0] == 0x52 && data[1] == 0x49 && data[2] == 0x46 && data[3] == 0x46 {
        return true;
    }
    // fLaC
    if data[0] == 0x66 && data[1] == 0x4c && data[2] == 0x61 && data[3] == 0x43 {
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

/// Tracks active downloads so duplicate requests coalesce.
struct ActiveDownload {
    notify: Arc<Notify>,
    result: Arc<Mutex<Option<Result<PathBuf, String>>>>,
}

pub struct TrackCacheState {
    pub audio_dir: PathBuf,
    pub client: reqwest::Client,
    active: Mutex<HashMap<String, ActiveDownload>>,
}

pub fn init(audio_dir: PathBuf) -> TrackCacheState {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .expect("failed to build reqwest client");

    TrackCacheState {
        audio_dir,
        client,
        active: Mutex::new(HashMap::new()),
    }
}

impl TrackCacheState {
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

    /// Download track fully, save to cache, return path.
    /// Coalesces concurrent requests for the same URN.
    pub async fn ensure_cached(
        &self,
        urn: &str,
        url: &str,
        session_id: Option<&str>,
    ) -> Result<String, String> {
        // Already cached?
        if let Some(path) = self.get_cache_path(urn) {
            println!("[TrackCache] hit: {urn}");
            return Ok(path);
        }

        // Check if another task is already downloading this URN
        let mut active = self.active.lock().await;
        if let Some(existing) = active.get(urn) {
            println!("[TrackCache] coalescing request for {urn}");
            let notify = existing.notify.clone();
            let result_slot = existing.result.clone();
            drop(active);
            notify.notified().await;
            let res = result_slot.lock().await;
            return match res.as_ref() {
                Some(Ok(path)) => Ok(path.to_string_lossy().into_owned()),
                Some(Err(e)) => Err(e.clone()),
                None => Err("download completed without result".into()),
            };
        }

        // Register this download
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

        let download_result = self.download(urn, url, session_id).await;

        // Store result and notify waiters
        {
            let mut slot = result_slot.lock().await;
            *slot = Some(download_result.clone());
        }
        notify.notify_waiters();

        // Remove from active
        self.active.lock().await.remove(urn);

        download_result.map(|p| p.to_string_lossy().into_owned())
    }

    async fn download(
        &self,
        urn: &str,
        url: &str,
        session_id: Option<&str>,
    ) -> Result<PathBuf, String> {
        println!("[TrackCache] downloading {urn} from {url}");
        let start = std::time::Instant::now();
        let retry_delays = [300u64, 800, 2000];
        let mut last_err = String::new();

        for attempt in 0..=retry_delays.len() {
            if attempt > 0 {
                eprintln!("[TrackCache] retry #{attempt} for {urn}: {last_err}");
            }

            let mut req = self.client.get(url);
            if let Some(sid) = session_id {
                req = req.header("x-session-id", sid);
            }

            match req.send().await {
                Ok(resp) => {
                    let final_url = resp.url().clone();
                    let status = resp.status();
                    if final_url.as_str() != url {
                        println!("[TrackCache] {urn} redirected → {final_url} (HTTP {status})");
                    }
                    if status.is_success() {
                        match resp.bytes().await {
                            Ok(bytes) => {
                                if !is_valid_audio(&bytes) {
                                    eprintln!("[TrackCache] invalid audio for {urn}: {} bytes from {final_url}", bytes.len());
                                    return Err("Invalid audio data".into());
                                }
                                let path = self.file_path(urn);
                                tokio::fs::write(&path, &bytes)
                                    .await
                                    .map_err(|e| format!("Cache write failed: {e}"))?;
                                let kb = bytes.len() / 1024;
                                let ms = start.elapsed().as_millis();
                                println!("[TrackCache] downloaded {urn} — {kb} KB in {ms}ms");
                                return Ok(path);
                            }
                            Err(e) => last_err = format!("body read: {e}"),
                        }
                    } else if status.as_u16() == 429 || status.as_u16() >= 500 {
                        last_err = format!("HTTP {status} from {final_url}");
                    } else {
                        eprintln!("[TrackCache] failed {urn}: HTTP {status} from {final_url}");
                        return Err(format!("HTTP {status}"));
                    }
                }
                Err(e) => last_err = format!("request: {e}"),
            }

            if attempt < retry_delays.len() {
                tokio::time::sleep(std::time::Duration::from_millis(retry_delays[attempt])).await;
            }
        }

        eprintln!("[TrackCache] gave up on {urn} after {} retries: {last_err}", retry_delays.len());
        Err(last_err)
    }

    pub fn cache_size(&self) -> u64 {
        let mut total = 0u64;
        if let Ok(entries) = std::fs::read_dir(&self.audio_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
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
                if entry.metadata().map(|m| m.is_file()).unwrap_or(false) {
                    std::fs::remove_file(entry.path()).ok();
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
                        // Remove invalid/small files
                        std::fs::remove_file(entry.path()).ok();
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
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let size = meta.len();
                        let accessed = meta
                            .accessed()
                            .or_else(|_| meta.modified())
                            .unwrap_or(std::time::UNIX_EPOCH);
                        total += size;
                        files.push((entry.path(), size, accessed));
                    }
                }
            }
        }

        if total <= limit_bytes {
            return;
        }

        let before = total;
        // Sort oldest first
        files.sort_by(|a, b| a.2.cmp(&b.2));

        let mut removed = 0u32;
        for (path, size, _) in files {
            if total <= limit_bytes {
                break;
            }
            if std::fs::remove_file(&path).is_ok() {
                total -= size;
                removed += 1;
            }
        }
        println!("[TrackCache] evicted {removed} files, freed {} MB", (before - total) / (1024 * 1024));
    }
}
