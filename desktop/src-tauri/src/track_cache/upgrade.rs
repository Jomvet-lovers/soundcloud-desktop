//! Фоновый upgrade трека до hq, когда первый успешный download был sq.
//!
//! После того как `ensure_cached` уже отдал sq-результат клиенту, мы можем
//! параллельно дотянуть hq-вариант через `/download` и заменить файл в кэше.
//! Плеер потом бесшовно переключается через `audio_swap_source`.

use std::path::PathBuf;

use tauri::Emitter;

use super::direct_download::try_download;
use super::state::{
    cache_filename, write_bytes_to_cache, DownloadSource, PlaybackQuality, TrackCacheState,
};

pub struct UpgradeRequest {
    pub urn: String,
    pub download_urls: Vec<String>,
    pub session_id: Option<String>,
    pub liked: bool,
}

impl TrackCacheState {
    /// Спавнит фоновую task; если для этого urn уже что-то запущено — abort'ит
    /// предыдущую (свежий request важнее).
    pub fn schedule_upgrade(&self, req: UpgradeRequest) {
        if req.download_urls.is_empty() {
            return;
        }
        let urn = req.urn.clone();
        if let Ok(mut map) = self.upgrades.lock() {
            if let Some(prev) = map.remove(&urn) {
                prev.abort();
            }
        }

        let state = self.clone();
        let urn_for_cleanup = urn.clone();
        let handle = tokio::spawn(async move {
            state.run_upgrade(req).await;
            if let Ok(mut map) = state.upgrades.lock() {
                map.remove(&urn_for_cleanup);
            }
        });

        if let Ok(mut map) = self.upgrades.lock() {
            map.insert(urn, handle.abort_handle());
        }
    }

    pub fn cancel_upgrade(&self, urn: &str) -> bool {
        let Ok(mut map) = self.upgrades.lock() else {
            return false;
        };
        if let Some(h) = map.remove(urn) {
            h.abort();
            println!("[TrackCache] upgrade cancelled for {urn}");
            true
        } else {
            false
        }
    }

    async fn run_upgrade(&self, req: UpgradeRequest) {
        let UpgradeRequest {
            urn,
            download_urls,
            session_id,
            liked,
        } = req;

        let start = std::time::Instant::now();
        println!("[TrackCache] upgrade: trying hq for {urn}");

        let result = match try_download(
            &self.direct_client,
            &download_urls,
            session_id.as_deref(),
            true,
        )
        .await
        {
            Some(r) => r,
            None => {
                println!("[TrackCache] upgrade: no hq candidate for {urn}");
                return;
            }
        };

        if !matches!(result.quality, PlaybackQuality::Hq) {
            println!("[TrackCache] upgrade: direct returned sq for {urn}, skipping");
            return;
        }

        let target_dir = if liked {
            &self.liked_dir
        } else {
            &self.audio_dir
        };
        let path = match write_bytes_to_cache(
            target_dir,
            &urn,
            &result.data,
            PlaybackQuality::Hq,
            DownloadSource::Direct,
        )
        .await
        {
            Ok(res) => res.path,
            Err(_) => {
                eprintln!("[TrackCache] upgrade: hq write failed for {urn}");
                return;
            }
        };

        let kb = std::fs::metadata(&path)
            .map(|m| m.len() / 1024)
            .unwrap_or(0);
        let ms = start.elapsed().as_millis();
        println!("[TrackCache] upgraded {urn} → hq — {kb} KB in {ms}ms");

        if let Some(app) = self.app_handle.as_ref() {
            let payload = serde_json::json!({
                "urn": urn,
                "path": path_to_string(&path),
                "quality": "hq",
                "filename": cache_filename(&urn, PlaybackQuality::Hq),
            });
            let _ = app.emit("track:quality-upgraded", payload);
        }
    }
}

fn path_to_string(p: &PathBuf) -> String {
    p.to_string_lossy().into_owned()
}
