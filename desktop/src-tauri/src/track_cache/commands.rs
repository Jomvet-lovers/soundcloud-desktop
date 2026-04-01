use tauri::State;

use crate::track_cache::state::TrackCacheState;

#[derive(serde::Deserialize)]
pub struct PreloadEntry {
    pub urn: String,
    pub url: String,
    pub session_id: Option<String>,
}

#[tauri::command]
pub async fn track_ensure_cached(
    urn: String,
    url: String,
    session_id: Option<String>,
    state: State<'_, TrackCacheState>,
) -> Result<String, String> {
    state
        .ensure_cached(&urn, &url, session_id.as_deref())
        .await
}

#[tauri::command]
pub fn track_is_cached(urn: String, state: State<'_, TrackCacheState>) -> bool {
    state.is_cached(&urn)
}

#[tauri::command]
pub fn track_get_cache_path(urn: String, state: State<'_, TrackCacheState>) -> Option<String> {
    state.get_cache_path(&urn)
}

#[tauri::command]
pub async fn track_preload(
    entries: Vec<PreloadEntry>,
    state: State<'_, TrackCacheState>,
) -> Result<(), String> {
    let mut queued = 0u32;
    for entry in entries {
        if state.is_cached(&entry.urn) {
            continue;
        }
        queued += 1;
        let client = state.client.clone();
        let audio_dir = state.audio_dir.clone();
        let urn = entry.urn;
        let url = entry.url;
        let session_id = entry.session_id;

        tokio::spawn(async move {
            println!("[TrackCache] preloading {urn} from {url}");
            let start = std::time::Instant::now();
            let mut req = client.get(&url);
            if let Some(sid) = &session_id {
                req = req.header("x-session-id", sid.as_str());
            }
            match req.send().await {
                Ok(resp) => {
                    let final_url = resp.url().clone();
                    let status = resp.status();
                    if final_url.as_str() != url {
                        println!("[TrackCache] preload {urn} redirected → {final_url}");
                    }
                    if !status.is_success() {
                        eprintln!("[TrackCache] preload {urn}: HTTP {status} from {final_url}");
                        return;
                    }
                    match resp.bytes().await {
                        Ok(bytes) => {
                            if bytes.len() < 8192 {
                                eprintln!("[TrackCache] preload {urn}: too small ({} bytes)", bytes.len());
                                return;
                            }
                            let filename = format!("{}.audio", urn.replace(':', "_"));
                            let path = audio_dir.join(filename);
                            tokio::fs::write(&path, &bytes).await.ok();
                            let kb = bytes.len() / 1024;
                            let ms = start.elapsed().as_millis();
                            println!("[TrackCache] preloaded {urn} — {kb} KB in {ms}ms");
                        }
                        Err(e) => eprintln!("[TrackCache] preload {urn}: body read: {e}"),
                    }
                }
                Err(e) => {
                    eprintln!("[TrackCache] preload {urn}: {e}");
                }
            }
        });
    }
    if queued > 0 {
        println!("[TrackCache] queued {queued} preloads");
    }
    Ok(())
}

#[tauri::command]
pub fn track_cache_size(state: State<'_, TrackCacheState>) -> u64 {
    state.cache_size()
}

#[tauri::command]
pub fn track_clear_cache(state: State<'_, TrackCacheState>) {
    state.clear_cache();
}

#[tauri::command]
pub fn track_list_cached(state: State<'_, TrackCacheState>) -> Vec<String> {
    state.list_cached_urns()
}

#[tauri::command]
pub fn track_enforce_cache_limit(limit_mb: u64, state: State<'_, TrackCacheState>) {
    state.enforce_limit(limit_mb);
}
