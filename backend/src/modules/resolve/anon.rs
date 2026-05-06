use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json::{Map, Value};
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::sc::ScClient;

const SC_HOME: &str = "https://soundcloud.com";
const SC_API_V2: &str = "https://api-v2.soundcloud.com";
const UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

static HYDRATION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#,
    )
    .expect("hydration regex")
});

pub struct AnonResolveClient {
    sc: ScClient,
    client_id: RwLock<Option<String>>,
}

impl AnonResolveClient {
    pub fn new(sc: ScClient) -> Self {
        Self {
            sc,
            client_id: RwLock::new(None),
        }
    }

    pub async fn resolve(&self, url: &str) -> AppResult<Value> {
        let cid = self.get_client_id().await?;
        let target = build_resolve_url(url, &cid);
        match self.fetch_json(&target).await {
            Ok(v) => Ok(v),
            Err(_) => {
                let new_cid = self.refresh_client_id().await?;
                let retry = build_resolve_url(url, &new_cid);
                self.fetch_json(&retry).await
            }
        }
    }

    async fn get_client_id(&self) -> AppResult<String> {
        if let Some(cid) = self.client_id.read().await.clone() {
            return Ok(cid);
        }
        self.refresh_client_id().await
    }

    async fn refresh_client_id(&self) -> AppResult<String> {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_static(UA));
        let bytes = self.sc.anon_get_via_relay_proxy(SC_HOME, h).await?;
        let html = String::from_utf8_lossy(&bytes);
        let cid = extract_client_id(&html)
            .ok_or_else(|| AppError::internal("client_id not found in soundcloud.com hydration"))?;
        *self.client_id.write().await = Some(cid.clone());
        tracing::info!("[anon-resolve] refreshed client_id");
        Ok(cid)
    }

    async fn fetch_json(&self, target_url: &str) -> AppResult<Value> {
        let bytes = self
            .sc
            .anon_get_via_relay_proxy(target_url, HeaderMap::new())
            .await?;
        serde_json::from_slice(&bytes)
            .map_err(|e| AppError::internal(format!("v2 resolve json: {e}")))
    }
}

fn build_resolve_url(url: &str, client_id: &str) -> String {
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("url", url)
        .append_pair("client_id", client_id)
        .finish();
    format!("{SC_API_V2}/resolve?{q}")
}

fn extract_client_id(html: &str) -> Option<String> {
    HYDRATION_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

pub fn normalize_v2_to_v1(value: &mut Value) {
    match value {
        Value::Object(obj) => {
            normalize_object(obj);
            for (_, v) in obj.iter_mut() {
                normalize_v2_to_v1(v);
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                normalize_v2_to_v1(v);
            }
        }
        _ => {}
    }
}

fn normalize_object(obj: &mut Map<String, Value>) {
    if !obj.contains_key("favoritings_count") {
        if let Some(v) = obj.get("likes_count").cloned() {
            obj.insert("favoritings_count".to_string(), v);
        }
    }
    if !matches!(obj.get("urn"), Some(Value::String(_))) {
        if let Some(urn) = synth_urn(obj) {
            obj.insert("urn".to_string(), Value::String(urn));
        }
    }
}

fn synth_urn(obj: &Map<String, Value>) -> Option<String> {
    let kind = obj.get("kind").and_then(|v| v.as_str())?;
    let segment = match kind {
        "track" => "tracks",
        "playlist" => "playlists",
        "user" => "users",
        "system-playlist" => "system-playlists",
        _ => return None,
    };
    let id = obj.get("id").and_then(|v| match v {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    })?;
    Some(format!("soundcloud:{segment}:{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HYDRATION: &str = r#"window.__sc_hydration = [{"hydratable":"apiClient","data":{"id":"JNsHQvoXu3CrVm6Jv30i95VRZQ7h8lXX","isExpiring":false}}];"#;

    const V2_TRACK_SAMPLE: &str = r#"{
        "id": 293,
        "kind": "track",
        "urn": "soundcloud:tracks:293",
        "title": "Flickermood",
        "permalink_url": "https://soundcloud.com/forss/flickermood",
        "user_id": 183,
        "duration": 213886,
        "likes_count": 2592,
        "playback_count": 962957,
        "comment_count": 397,
        "reposts_count": 412,
        "media": {"transcodings": []},
        "user": {
            "id": 183,
            "kind": "user",
            "urn": "soundcloud:users:183",
            "username": "Forss",
            "likes_count": 289,
            "followers_count": 132197
        }
    }"#;

    const V2_PLAYLIST_SAMPLE: &str = r#"{
        "id": 1,
        "kind": "playlist",
        "urn": "soundcloud:playlists:1",
        "title": "Mix",
        "user_id": 5,
        "likes_count": 10,
        "tracks": [
            {"id": 100, "kind": "track", "urn": "soundcloud:tracks:100", "likes_count": 50}
        ]
    }"#;

    #[test]
    fn extract_client_id_from_hydration() {
        let cid = extract_client_id(SAMPLE_HYDRATION).expect("found");
        assert_eq!(cid, "JNsHQvoXu3CrVm6Jv30i95VRZQ7h8lXX");
    }

    #[test]
    fn extract_client_id_missing_returns_none() {
        assert!(extract_client_id("nothing here").is_none());
    }

    #[test]
    fn build_resolve_url_encodes() {
        let u = build_resolve_url("https://soundcloud.com/a/b?x=1", "CID");
        assert!(u.starts_with("https://api-v2.soundcloud.com/resolve?"));
        assert!(u.contains("url=https%3A%2F%2Fsoundcloud.com%2Fa%2Fb%3Fx%3D1"));
        assert!(u.contains("client_id=CID"));
    }

    #[test]
    fn normalize_track_aliases_likes_count() {
        let mut v: Value = serde_json::from_str(V2_TRACK_SAMPLE).unwrap();
        normalize_v2_to_v1(&mut v);
        assert_eq!(v["kind"], "track");
        assert_eq!(v["urn"], "soundcloud:tracks:293");
        assert_eq!(v["favoritings_count"], 2592);
        assert_eq!(v["likes_count"], 2592);
        assert_eq!(v["user"]["favoritings_count"], 289);
    }

    #[test]
    fn normalize_playlist_recurses_into_tracks() {
        let mut v: Value = serde_json::from_str(V2_PLAYLIST_SAMPLE).unwrap();
        normalize_v2_to_v1(&mut v);
        assert_eq!(v["favoritings_count"], 10);
        assert_eq!(v["tracks"][0]["favoritings_count"], 50);
    }

    #[test]
    fn normalize_synthesizes_playlist_urn() {
        let mut v: Value = serde_json::json!({
            "id": 1972920441u64,
            "kind": "playlist",
            "title": "Mix",
            "likes_count": 3216,
        });
        normalize_v2_to_v1(&mut v);
        assert_eq!(v["urn"], "soundcloud:playlists:1972920441");
        assert_eq!(v["favoritings_count"], 3216);
    }

    #[test]
    fn normalize_keeps_existing_urn() {
        let mut v: Value = serde_json::json!({
            "id": 1,
            "kind": "track",
            "urn": "soundcloud:tracks:custom",
        });
        normalize_v2_to_v1(&mut v);
        assert_eq!(v["urn"], "soundcloud:tracks:custom");
    }

    #[test]
    fn normalize_handles_string_id_for_system_playlist() {
        let mut v: Value = serde_json::json!({
            "id": "charts-top:all-music",
            "kind": "system-playlist",
        });
        normalize_v2_to_v1(&mut v);
        assert_eq!(
            v["urn"],
            "soundcloud:system-playlists:charts-top:all-music"
        );
    }

    #[test]
    fn normalize_preserves_existing_favoritings_count() {
        let mut v: Value = serde_json::json!({
            "kind": "track",
            "likes_count": 10,
            "favoritings_count": 99
        });
        normalize_v2_to_v1(&mut v);
        assert_eq!(v["favoritings_count"], 99);
    }
}
