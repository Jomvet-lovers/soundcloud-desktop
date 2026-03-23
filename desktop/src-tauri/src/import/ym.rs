use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

static CANCEL_FLAG: std::sync::LazyLock<Arc<AtomicBool>> =
    std::sync::LazyLock::new(|| Arc::new(AtomicBool::new(false)));

#[derive(serde::Serialize, Clone)]
pub struct YmImportProgress {
    pub total: usize,
    pub current: usize,
    pub found: usize,
    pub not_found: usize,
    pub current_track: String,
}

#[derive(serde::Deserialize)]
struct YmLikesResponse {
    result: YmLikesResult,
}

#[derive(serde::Deserialize)]
struct YmLikesResult {
    library: YmLibrary,
}

#[derive(serde::Deserialize)]
struct YmLibrary {
    tracks: Vec<YmLikedTrack>,
}

#[derive(serde::Deserialize)]
struct YmLikedTrack {
    id: serde_json::Value,
}

#[derive(serde::Deserialize)]
struct YmTrackInfo {
    result: Vec<YmTrack>,
}

#[derive(serde::Deserialize)]
struct YmTrack {
    title: Option<String>,
    artists: Option<Vec<YmArtist>>,
}

#[derive(serde::Deserialize)]
struct YmArtist {
    name: Option<String>,
}

#[derive(serde::Deserialize)]
struct ScSearchResult {
    collection: Vec<ScTrackResult>,
}

#[derive(serde::Deserialize)]
struct ScTrackResult {
    urn: Option<String>,
}

#[tauri::command]
pub async fn ym_import_start(
    ym_token: String,
    backend_url: String,
    session_id: String,
    app: AppHandle,
) -> Result<Vec<String>, String> {
    CANCEL_FLAG.store(false, Ordering::Relaxed);

    let client = reqwest::Client::new();

    let uid_resp = client
        .get("https://api.music.yandex.net/account/status")
        .header("Authorization", format!("OAuth {}", ym_token))
        .send()
        .await
        .map_err(|e| format!("YM auth failed: {}", e))?;

    if !uid_resp.status().is_success() {
        return Err(format!("YM auth failed: HTTP {}", uid_resp.status()));
    }

    let uid_json: serde_json::Value = uid_resp.json().await.map_err(|e| e.to_string())?;
    let uid = uid_json["result"]["account"]["uid"]
        .as_i64()
        .ok_or("Failed to get YM user ID")?;

    let likes_resp = client
        .get(format!(
            "https://api.music.yandex.net/users/{}/likes/tracks",
            uid
        ))
        .header("Authorization", format!("OAuth {}", ym_token))
        .send()
        .await
        .map_err(|e| format!("Failed to fetch YM likes: {}", e))?;

    let likes: YmLikesResponse = likes_resp.json().await.map_err(|e| e.to_string())?;
    let track_ids: Vec<String> = likes
        .result
        .library
        .tracks
        .iter()
        .map(|t| match &t.id {
            serde_json::Value::Number(n) => n.to_string(),
            serde_json::Value::String(s) => s.clone(),
            v => v.to_string(),
        })
        .collect();

    let total = track_ids.len();
    let mut found = 0usize;
    let mut not_found = 0usize;
    let mut found_urns: Vec<String> = Vec::new();

    for (batch_idx, chunk) in track_ids.chunks(50).enumerate() {
        if CANCEL_FLAG.load(Ordering::Relaxed) {
            break;
        }

        let ids_param = chunk.join(",");
        let info_resp = client
            .get(format!(
                "https://api.music.yandex.net/tracks?trackIds={}",
                ids_param
            ))
            .header("Authorization", format!("OAuth {}", ym_token))
            .send()
            .await;

        let tracks: Vec<YmTrack> = match info_resp {
            Ok(r) => match r.json::<YmTrackInfo>().await {
                Ok(info) => info.result,
                Err(_) => continue,
            },
            Err(_) => continue,
        };

        for (i, track) in tracks.iter().enumerate() {
            if CANCEL_FLAG.load(Ordering::Relaxed) {
                break;
            }

            let title = track.title.as_deref().unwrap_or("");
            let artist = track
                .artists
                .as_ref()
                .and_then(|a: &Vec<YmArtist>| a.first())
                .and_then(|a| a.name.as_deref())
                .unwrap_or("");

            if title.is_empty() && artist.is_empty() {
                not_found += 1;
                continue;
            }

            let current = batch_idx * 50 + i + 1;
            let current_track = format!("{} - {}", artist, title);

            app.emit(
                "ym_import:progress",
                YmImportProgress {
                    total,
                    current,
                    found,
                    not_found,
                    current_track: current_track.clone(),
                },
            )
            .ok();

            let query = format!("{} {}", artist, title);
            let search_url = format!(
                "{}/tracks?q={}&limit=3&linked_partitioning=true",
                backend_url,
                urlencoding::encode(&query)
            );

            let search_resp = client
                .get(&search_url)
                .header("x-session-id", &session_id)
                .send()
                .await;

            if let Ok(resp) = search_resp {
                if let Ok(results) = resp.json::<ScSearchResult>().await {
                    if let Some(urn) = results.collection.first().and_then(|t| t.urn.as_deref()) {
                        found_urns.push(urn.to_string());
                        found += 1;
                    } else {
                        not_found += 1;
                    }
                } else {
                    not_found += 1;
                }
            } else {
                not_found += 1;
            }

            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }

    app.emit(
        "ym_import:progress",
        YmImportProgress {
            total,
            current: total,
            found,
            not_found,
            current_track: String::new(),
        },
    )
    .ok();

    Ok(found_urns)
}

#[tauri::command]
pub fn ym_import_stop() {
    CANCEL_FLAG.store(true, Ordering::Relaxed);
}
