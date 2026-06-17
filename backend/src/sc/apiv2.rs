//! Channel B: apiv2 via proxy&relay (`anon_get_via_relay_proxy`). It is the fallback for
//! channel A (the Lua method via the relay) and implements the same multi-step flows in
//! Rust — playlist hydration, collection paging, search.

use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::header::{HeaderMap, HeaderValue, USER_AGENT};
use serde_json::Value;
use tokio::sync::RwLock;

use crate::error::{AppError, AppResult};
use crate::sc::mapping::{
    self, collect_playlist_track_ids, index_tracks_by_id, reassemble_playlist_tracks,
    PublicCollection, SearchType,
};
use crate::sc::ScClient;

const SC_HOME: &str = "https://soundcloud.com";
const SC_API_V2: &str = "https://api-v2.soundcloud.com";
const UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";
const HYDRATE_CHUNK: usize = 50;

static HYDRATION_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#""hydratable"\s*:\s*"apiClient"\s*,\s*"data"\s*:\s*\{\s*"id"\s*:\s*"([^"]+)""#)
        .expect("hydration regex")
});

/// One page of a collection/search read.
pub struct Page {
    /// Raw apiv2 collection items (still wrapped for like-feeds — the caller unwraps).
    pub items: Vec<Value>,
    pub next_href: Option<String>,
}

/// apiv2 reads via proxy&relay (client_id scraped from the SC homepage).
pub struct Apiv2Proxy {
    sc: ScClient,
    client_id: RwLock<Option<String>>,
}

impl Apiv2Proxy {
    pub fn new(sc: ScClient) -> Self {
        Self {
            sc,
            client_id: RwLock::new(None),
        }
    }

    /// `/resolve?url=…` → raw apiv2 entity.
    pub async fn resolve(&self, url: &str) -> AppResult<Value> {
        self.get_with_retry(|cid| {
            let q = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("url", url)
                .append_pair("client_id", cid)
                .finish();
            format!("{SC_API_V2}/resolve?{q}")
        })
        .await
    }

    pub async fn track(&self, sc_track_id: &str) -> AppResult<Value> {
        self.get_with_retry(|cid| format!("{SC_API_V2}/tracks/{sc_track_id}?client_id={cid}"))
            .await
    }

    pub async fn user(&self, user_id: &str) -> AppResult<Value> {
        self.get_with_retry(|cid| format!("{SC_API_V2}/users/{user_id}?client_id={cid}"))
            .await
    }

    /// Playlist (+ optionally its full ordered track list): fetch the playlist, then,
    /// when `hydrate`, batch-hydrate stub track ids via `/tracks?ids=…` and reassemble
    /// in order. `hydrate=false` returns meta only (skips the `/tracks` batches).
    pub async fn playlist(&self, playlist_id: &str, hydrate: bool) -> AppResult<Value> {
        let mut playlist = self
            .get_with_retry(|cid| format!("{SC_API_V2}/playlists/{playlist_id}?client_id={cid}"))
            .await?;
        if !hydrate {
            mapping::normalize_v2_to_v1(&mut playlist);
            return Ok(playlist);
        }
        let (ids, embedded) = collect_playlist_track_ids(&playlist);
        let missing: Vec<&String> = ids
            .iter()
            .filter(|id| !embedded.contains_key(*id))
            .collect();

        let mut hydrated = std::collections::HashMap::new();
        for chunk in missing.chunks(HYDRATE_CHUNK) {
            let id_list = chunk
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(",");
            let arr = self
                .get_with_retry(|cid| format!("{SC_API_V2}/tracks?ids={id_list}&client_id={cid}"))
                .await?;
            if let Some(items) = arr.as_array() {
                hydrated.extend(index_tracks_by_id(items));
            }
        }

        let tracks = reassemble_playlist_tracks(&ids, &embedded, &hydrated);
        mapping::normalize_v2_to_v1(&mut playlist);
        if let Some(obj) = playlist.as_object_mut() {
            obj.insert("tracks".to_string(), Value::Array(tracks));
        }
        Ok(playlist)
    }

    /// One page of a public per-user collection. `cursor` is a prior `next_href`
    /// (SC omits client_id from it, so we always re-append ours).
    pub async fn collection_page(
        &self,
        coll: PublicCollection,
        user_id: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<Page> {
        let seg = coll.path_segment();
        let page = self
            .get_with_retry(|cid| match cursor {
                Some(c) => with_client_id(c, cid),
                None => format!(
                    "{SC_API_V2}/users/{user_id}/{seg}?client_id={cid}&limit={limit}&linked_partitioning=true"
                ),
            })
            .await?;
        Ok(parse_page(&page))
    }

    pub async fn search_page(
        &self,
        ty: SearchType,
        q: &str,
        cursor: Option<&str>,
        limit: i64,
    ) -> AppResult<Page> {
        let seg = ty.as_str();
        let page = self
            .get_with_retry(|cid| match cursor {
                Some(c) => with_client_id(c, cid),
                None => {
                    let qq = url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>();
                    format!(
                        "{SC_API_V2}/search/{seg}?client_id={cid}&q={qq}&limit={limit}&linked_partitioning=true"
                    )
                }
            })
            .await?;
        Ok(parse_page(&page))
    }

    /// Generic list GET of a full api-v2 URL (without client_id) → one page. For public
    /// paginated lists (comments/reposters/related/followers) + cron list-walks.
    pub async fn get_list(&self, url: &str) -> AppResult<Page> {
        let page = self.get_with_retry(|cid| with_client_id(url, cid)).await?;
        Ok(parse_page(&page))
    }

    /// Run `build(client_id)` → GET JSON; on failure refresh the client_id once and retry
    /// (a stale scraped client_id is the common transient).
    async fn get_with_retry<F>(&self, build: F) -> AppResult<Value>
    where
        F: Fn(&str) -> String,
    {
        let cid = self.get_client_id().await?;
        match self.get_json(&build(&cid)).await {
            Ok(v) => Ok(v),
            Err(_) => {
                let cid = self.refresh_client_id().await?;
                self.get_json(&build(&cid)).await
            }
        }
    }

    async fn get_json(&self, target_url: &str) -> AppResult<Value> {
        let bytes = self
            .sc
            .anon_get_via_relay_proxy(target_url, HeaderMap::new())
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| AppError::internal(format!("apiv2 json: {e}")))
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
        tracing::info!("[apiv2-proxy] refreshed client_id");
        Ok(cid)
    }
}

fn with_client_id(u: &str, cid: &str) -> String {
    if u.contains('?') {
        format!("{u}&client_id={cid}")
    } else {
        format!("{u}?client_id={cid}")
    }
}

fn parse_page(page: &Value) -> Page {
    let items = page
        .get("collection")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let next_href = page
        .get("next_href")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(String::from);
    Page { items, next_href }
}

fn extract_client_id(html: &str) -> Option<String> {
    HYDRATION_RE
        .captures(html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_HYDRATION: &str = r#"window.__sc_hydration = [{"hydratable":"apiClient","data":{"id":"JNsHQvoXu3CrVm6Jv30i95VRZQ7h8lXX","isExpiring":false}}];"#;

    #[test]
    fn extract_client_id_from_hydration() {
        assert_eq!(
            extract_client_id(SAMPLE_HYDRATION).as_deref(),
            Some("JNsHQvoXu3CrVm6Jv30i95VRZQ7h8lXX")
        );
    }

    #[test]
    fn with_client_id_appends_correctly() {
        assert_eq!(
            with_client_id("https://x/y", "C"),
            "https://x/y?client_id=C"
        );
        assert_eq!(
            with_client_id("https://x/y?offset=z", "C"),
            "https://x/y?offset=z&client_id=C"
        );
    }

    #[test]
    fn parse_page_extracts_fields() {
        let v = serde_json::json!({
            "collection": [{"id": 1}, {"id": 2}],
            "next_href": "https://api-v2.soundcloud.com/x?offset=2",
            "total_results": 99
        });
        let p = parse_page(&v);
        assert_eq!(p.items.len(), 2);
        assert_eq!(
            p.next_href.as_deref(),
            Some("https://api-v2.soundcloud.com/x?offset=2")
        );
    }

    #[test]
    fn parse_page_empty_next_href_is_none() {
        let v = serde_json::json!({"collection": [], "next_href": ""});
        let p = parse_page(&v);
        assert!(p.items.is_empty());
        assert!(p.next_href.is_none());
    }
}
