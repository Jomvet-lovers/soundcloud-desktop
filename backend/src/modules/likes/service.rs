use std::sync::Arc;
use std::time::Duration;

use mini_moka::sync::Cache;
use serde_json::{json, Value};
use tokio::sync::Mutex as AsyncMutex;

use crate::error::{AppError, AppResult};
use crate::modules::local_likes::LocalLikesService;
use crate::modules::pending_actions::PendingActionsService;
use crate::sc::ScClient;

const LIKES_PLAYLIST_NAME: &str = "Лайки | SoundCloud Desktop";
const LIKE_RETRY_ATTEMPTS: u32 = 3;
const LIKE_RETRY_DELAY_MS: u64 = 500;
const PLAYLIST_CACHE_TTL: Duration = Duration::from_secs(5 * 60);
const PLAYLIST_TRACKS_CACHE_TTL: Duration = Duration::from_secs(30);
const SYNC_LOCK_CAPACITY: u64 = 8192;
const SYNC_LOCK_TTL: Duration = Duration::from_secs(10 * 60);
const PLAYLIST_CACHE_CAPACITY: u64 = 8192;

pub struct LikesService {
    sc: ScClient,
    local_likes: Arc<LocalLikesService>,
    pending: Arc<PendingActionsService>,
    likes_playlist_cache: Cache<String, Value>,
    playlist_tracks_cache: Cache<String, Vec<String>>,
    sync_locks: Cache<String, Arc<AsyncMutex<()>>>,
}

impl LikesService {
    pub fn new(
        sc: ScClient,
        local_likes: Arc<LocalLikesService>,
        pending: Arc<PendingActionsService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            sc,
            local_likes,
            pending,
            likes_playlist_cache: Cache::builder()
                .max_capacity(PLAYLIST_CACHE_CAPACITY)
                .time_to_live(PLAYLIST_CACHE_TTL)
                .build(),
            playlist_tracks_cache: Cache::builder()
                .max_capacity(PLAYLIST_CACHE_CAPACITY)
                .time_to_live(PLAYLIST_TRACKS_CACHE_TTL)
                .build(),
            sync_locks: Cache::builder()
                .max_capacity(SYNC_LOCK_CAPACITY)
                .time_to_idle(SYNC_LOCK_TTL)
                .build(),
        })
    }

    fn lock_for(&self, key: &str) -> Arc<AsyncMutex<()>> {
        if let Some(lock) = self.sync_locks.get(&key.to_string()) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        self.sync_locks.insert(key.to_string(), lock.clone());
        lock
    }

    async fn retry_like_track(&self, token: &str, track_urn: &str) -> AppResult<Value> {
        let mut last_err: Option<AppError> = None;
        for attempt in 1..=LIKE_RETRY_ATTEMPTS {
            match self
                .sc
                .api_post::<Value, Value>(&format!("/likes/tracks/{track_urn}"), token, None)
                .await
            {
                Ok(v) => return Ok(v),
                Err(e) => {
                    last_err = Some(e);
                    if attempt < LIKE_RETRY_ATTEMPTS {
                        tokio::time::sleep(Duration::from_millis(
                            LIKE_RETRY_DELAY_MS * attempt as u64,
                        ))
                        .await;
                    }
                }
            }
        }
        Err(last_err.unwrap_or_else(|| AppError::internal("retry failed")))
    }

    async fn find_likes_playlist(&self, token: &str) -> AppResult<Option<Value>> {
        let mut cursor: Option<String> = None;
        loop {
            let mut params: Vec<(String, String)> = vec![
                ("limit".into(), "200".into()),
                ("linked_partitioning".into(), "true".into()),
            ];
            if let Some(c) = &cursor {
                params.push(("cursor".into(), c.clone()));
            }
            let page: Value = self.sc.api_get_value("/me/playlists", token, Some(&params)).await?;
            if let Some(items) = page.get("collection").and_then(|v| v.as_array()) {
                for item in items {
                    if let Some(title) = item.get("title").and_then(|v| v.as_str()) {
                        if title.trim() == LIKES_PLAYLIST_NAME {
                            return Ok(Some(item.clone()));
                        }
                    }
                }
            }
            let Some(href) = page.get("next_href").and_then(|v| v.as_str()).map(String::from) else {
                return Ok(None);
            };
            match extract_cursor(&href) {
                Some(c) if Some(&c) != cursor.as_ref() => cursor = Some(c),
                _ => return Ok(None),
            }
        }
    }

    async fn ensure_likes_playlist(&self, session_id: &str, token: &str) -> AppResult<Value> {
        let cache_key = format!("likes-playlist:{session_id}");
        if let Some(v) = self.likes_playlist_cache.get(&cache_key) {
            return Ok(v);
        }
        if let Some(existing) = self.find_likes_playlist(token).await? {
            self.likes_playlist_cache.insert(cache_key, existing.clone());
            return Ok(existing);
        }
        let body = json!({
            "playlist": { "title": LIKES_PLAYLIST_NAME, "sharing": "private" }
        });
        let created = self.sc.api_post_value("/playlists", token, Some(&body)).await?;
        self.likes_playlist_cache.insert(cache_key, created.clone());
        if let Some(urn) = created.get("urn").and_then(|v| v.as_str()) {
            self.playlist_tracks_cache
                .insert(format!("likes-playlist-tracks:{session_id}:{urn}"), Vec::new());
        }
        Ok(created)
    }

    async fn fetch_all_playlist_track_urns(
        &self,
        token: &str,
        playlist_urn: &str,
    ) -> AppResult<Vec<String>> {
        let mut cursor: Option<String> = None;
        let mut urns: Vec<String> = Vec::new();
        loop {
            let mut params: Vec<(String, String)> = vec![
                ("limit".into(), "200".into()),
                ("linked_partitioning".into(), "true".into()),
            ];
            if let Some(c) = &cursor {
                params.push(("cursor".into(), c.clone()));
            }
            let page: Value = self
                .sc
                .api_get_value(&format!("/playlists/{playlist_urn}/tracks"), token, Some(&params))
                .await?;
            if let Some(items) = page.get("collection").and_then(|v| v.as_array()) {
                for t in items {
                    if let Some(u) = t.get("urn").and_then(|v| v.as_str()) {
                        urns.push(u.to_string());
                    }
                }
            }
            let Some(href) = page.get("next_href").and_then(|v| v.as_str()).map(String::from) else {
                return Ok(urns);
            };
            match extract_cursor(&href) {
                Some(c) if Some(&c) != cursor.as_ref() => cursor = Some(c),
                _ => return Ok(urns),
            }
        }
    }

    async fn get_playlist_track_urns_cached(
        &self,
        session_id: &str,
        token: &str,
        playlist_urn: &str,
        force_refresh: bool,
    ) -> AppResult<Vec<String>> {
        let cache_key = format!("likes-playlist-tracks:{session_id}:{playlist_urn}");
        if !force_refresh {
            if let Some(v) = self.playlist_tracks_cache.get(&cache_key) {
                return Ok(v);
            }
        }
        let urns = self.fetch_all_playlist_track_urns(token, playlist_urn).await?;
        self.playlist_tracks_cache.insert(cache_key, urns.clone());
        Ok(urns)
    }

    fn invalidate_likes_playlist_cache(&self, session_id: &str, playlist_urn: Option<&str>) {
        self.likes_playlist_cache
            .invalidate(&format!("likes-playlist:{session_id}"));
        if let Some(urn) = playlist_urn {
            self.playlist_tracks_cache
                .invalidate(&format!("likes-playlist-tracks:{session_id}:{urn}"));
        }
    }

    async fn update_likes_playlist_tracks(
        &self,
        session_id: &str,
        token: &str,
        playlist_urn: &str,
        track_urns: Vec<String>,
    ) -> AppResult<Value> {
        let body = json!({
            "playlist": {
                "tracks": track_urns.iter().map(|u| json!({ "urn": u })).collect::<Vec<_>>()
            }
        });
        let updated = self
            .sc
            .api_put_value(&format!("/playlists/{playlist_urn}"), token, Some(&body))
            .await?;
        self.playlist_tracks_cache
            .insert(format!("likes-playlist-tracks:{session_id}:{playlist_urn}"), track_urns);
        self.likes_playlist_cache
            .insert(format!("likes-playlist:{session_id}"), updated.clone());
        Ok(updated)
    }

    async fn sync_track_with_likes_playlist_once(
        &self,
        session_id: &str,
        token: &str,
        track_urn: &str,
        should_be_present: bool,
    ) -> AppResult<Value> {
        let playlist = self.ensure_likes_playlist(session_id, token).await?;
        let urn = playlist
            .get("urn")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::internal("likes playlist has no urn"))?
            .to_string();

        let existing = self
            .get_playlist_track_urns_cached(session_id, token, &urn, false)
            .await?;
        let has_track = existing.iter().any(|u| u == track_urn);

        if (should_be_present && has_track) || (!should_be_present && !has_track) {
            return Ok(playlist);
        }

        let next: Vec<String> = if should_be_present {
            std::iter::once(track_urn.to_string()).chain(existing.into_iter()).collect()
        } else {
            existing.into_iter().filter(|u| u != track_urn).collect()
        };

        self.update_likes_playlist_tracks(session_id, token, &urn, next).await
    }

    async fn sync_track_with_likes_playlist(
        &self,
        session_id: &str,
        token: &str,
        track_urn: &str,
        should_be_present: bool,
    ) -> AppResult<Value> {
        match self
            .sync_track_with_likes_playlist_once(session_id, token, track_urn, should_be_present)
            .await
        {
            Ok(v) => Ok(v),
            Err(_) => {
                let cached_urn = self
                    .likes_playlist_cache
                    .get(&format!("likes-playlist:{session_id}"))
                    .and_then(|p| p.get("urn").and_then(|v| v.as_str()).map(String::from));
                self.invalidate_likes_playlist_cache(session_id, cached_urn.as_deref());

                let playlist = self.ensure_likes_playlist(session_id, token).await?;
                let urn = playlist
                    .get("urn")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| AppError::internal("likes playlist has no urn"))?
                    .to_string();
                let existing = self
                    .get_playlist_track_urns_cached(session_id, token, &urn, true)
                    .await?;
                let has_track = existing.iter().any(|u| u == track_urn);
                if (should_be_present && has_track) || (!should_be_present && !has_track) {
                    return Ok(playlist);
                }
                let next: Vec<String> = if should_be_present {
                    std::iter::once(track_urn.to_string()).chain(existing.into_iter()).collect()
                } else {
                    existing.into_iter().filter(|u| u != track_urn).collect()
                };
                self.update_likes_playlist_tracks(session_id, token, &urn, next).await
            }
        }
    }

    async fn track_data_for_fallback(
        &self,
        token: &str,
        track_urn: &str,
        track_data: Option<&Value>,
    ) -> Option<Value> {
        if let Some(td) = track_data {
            return Some(td.clone());
        }
        self.sc
            .api_get_value(&format!("/tracks/{track_urn}"), token, None)
            .await
            .ok()
    }

    pub async fn like_track(
        &self,
        token: &str,
        sc_user_id: &str,
        session_id: &str,
        track_urn: &str,
        track_data: Option<&Value>,
    ) -> AppResult<Value> {
        let lock = self.lock_for(&format!("likes-sync:{session_id}"));
        let _g = lock.lock().await;

        let sc_like_ok = self.retry_like_track(token, track_urn).await.is_ok();
        let playlist_sync_ok = self
            .sync_track_with_likes_playlist(session_id, token, track_urn, true)
            .await
            .is_ok();

        if !sc_like_ok || !playlist_sync_ok {
            if let Some(td) = self.track_data_for_fallback(token, track_urn, track_data).await {
                let _ = self.local_likes.add(sc_user_id, track_urn, &td).await;
            }
            if !sc_like_ok && !playlist_sync_ok {
                return Ok(json!({ "status": "local" }));
            }
            return Ok(json!({
                "status": "synced_with_fallback",
                "soundcloud": sc_like_ok,
                "playlist": playlist_sync_ok,
            }));
        }

        let _ = self.local_likes.remove(sc_user_id, track_urn).await;
        Ok(json!({ "status": "ok", "playlist": true }))
    }

    pub async fn unlike_track(
        &self,
        token: &str,
        sc_user_id: &str,
        session_id: &str,
        track_urn: &str,
    ) -> AppResult<Value> {
        let lock = self.lock_for(&format!("likes-sync:{session_id}"));
        let _g = lock.lock().await;

        let path = format!("/likes/tracks/{track_urn}");
        let sc_fut = self.sc.api_delete(&path, token);
        let pl_fut = self.sync_track_with_likes_playlist(session_id, token, track_urn, false);
        let ll_fut = self.local_likes.remove(sc_user_id, track_urn);
        let (sc_res, _, _) = tokio::join!(sc_fut, pl_fut, ll_fut);
        match sc_res {
            Ok(v) => Ok(v),
            Err(_) => Ok(json!({ "status": "removed" })),
        }
    }

    pub async fn like_playlist(
        &self,
        token: &str,
        session_id: &str,
        playlist_urn: &str,
    ) -> AppResult<Value> {
        match self
            .sc
            .api_post::<Value, Value>(&format!("/likes/playlists/{playlist_urn}"), token, None)
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "like_playlist", playlist_urn, None)
                    .await?;
                Ok(json!({
                    "queued": true,
                    "actionType": "like_playlist",
                    "targetUrn": playlist_urn,
                }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn unlike_playlist(
        &self,
        token: &str,
        session_id: &str,
        playlist_urn: &str,
    ) -> AppResult<Value> {
        match self.sc.api_delete(&format!("/likes/playlists/{playlist_urn}"), token).await {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "unlike_playlist", playlist_urn, None)
                    .await?;
                Ok(json!({
                    "queued": true,
                    "actionType": "unlike_playlist",
                    "targetUrn": playlist_urn,
                }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn is_playlist_liked(&self, token: &str, playlist_urn: &str) -> AppResult<Value> {
        let mut cursor: Option<String> = None;
        loop {
            let mut params: Vec<(String, String)> = vec![
                ("limit".into(), "200".into()),
                ("linked_partitioning".into(), "true".into()),
            ];
            if let Some(c) = &cursor {
                params.push(("cursor".into(), c.clone()));
            }
            let page: Value = self
                .sc
                .api_get_value("/me/likes/playlists", token, Some(&params))
                .await?;
            let Some(items) = page.get("collection").and_then(|v| v.as_array()) else {
                break;
            };
            if items.iter().any(|p| p.get("urn").and_then(|v| v.as_str()) == Some(playlist_urn)) {
                return Ok(json!({ "liked": true }));
            }
            let Some(href) = page.get("next_href").and_then(|v| v.as_str()).map(String::from) else {
                break;
            };
            match extract_cursor(&href) {
                Some(c) if Some(&c) != cursor.as_ref() => cursor = Some(c),
                _ => break,
            }
        }
        Ok(json!({ "liked": false }))
    }
}

fn extract_cursor(href: &str) -> Option<String> {
    let url = url::Url::parse(href).ok()?;
    url.query_pairs()
        .find(|(k, _)| k == "cursor")
        .map(|(_, v)| v.into_owned())
}
