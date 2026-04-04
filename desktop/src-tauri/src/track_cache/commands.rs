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

        let Some(permit) = state.try_acquire_preload_slot() else {
            continue;
        };

        queued += 1;
        let state = state.inner().clone();
        let urn = entry.urn;
        let url = entry.url;
        let session_id = entry.session_id;

        tokio::spawn(async move {
            let _permit = permit;
            println!("[TrackCache] preloading {urn} from {url}");
            if let Err(err) = state
                .ensure_cached(&urn, &url, session_id.as_deref())
                .await
            {
                eprintln!("[TrackCache] preload {urn}: {err}");
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
