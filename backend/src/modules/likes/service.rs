use std::sync::Arc;

use serde_json::{json, Value};
use sqlx::PgPool;

use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;
use crate::modules::cold_refresh::upsert_track_cache;
use crate::modules::sync_queue::mirror::{self, LIKES_PLAYLISTS, LIKES_TRACKS};
use crate::modules::sync_queue::SyncQueueService;

pub struct LikesService {
    pg: PgPool,
    sync_queue: Arc<SyncQueueService>,
}

impl LikesService {
    pub fn new(pg: PgPool, sync_queue: Arc<SyncQueueService>) -> Arc<Self> {
        Arc::new(Self { pg, sync_queue })
    }

    /// Оптимистичный лайк трека. Если в body приехал track_data — заодно
    /// прогреваем indexed_tracks, чтобы холодное чтение /me/likes/tracks имело
    /// payload без захода в SC.
    pub async fn like_track(
        &self,
        sc_user_id: &str,
        track_urn: &str,
        track_data: Option<&Value>,
    ) -> AppResult<Value> {
        let sc_track_id = extract_sc_id(track_urn);
        if let Some(td) = track_data {
            upsert_track_cache(&self.pg, sc_track_id, td).await?;
        }
        mirror::set_wanted(&self.pg, LIKES_TRACKS, sc_user_id, sc_track_id).await?;
        self.sync_queue
            .enqueue(sc_user_id, "like_track", track_urn, None)
            .await?;
        Ok(json!({ "status": "queued", "actionType": "like_track" }))
    }

    pub async fn unlike_track(&self, sc_user_id: &str, track_urn: &str) -> AppResult<Value> {
        let sc_track_id = extract_sc_id(track_urn);
        mirror::clear_wanted(&self.pg, LIKES_TRACKS, sc_user_id, sc_track_id).await?;
        self.sync_queue
            .enqueue(sc_user_id, "unlike_track", track_urn, None)
            .await?;
        Ok(json!({ "status": "queued", "actionType": "unlike_track" }))
    }

    pub async fn like_playlist(&self, sc_user_id: &str, playlist_urn: &str) -> AppResult<Value> {
        mirror::set_wanted(&self.pg, LIKES_PLAYLISTS, sc_user_id, playlist_urn).await?;
        self.sync_queue
            .enqueue(sc_user_id, "like_playlist", playlist_urn, None)
            .await?;
        Ok(json!({ "status": "queued", "actionType": "like_playlist" }))
    }

    pub async fn unlike_playlist(&self, sc_user_id: &str, playlist_urn: &str) -> AppResult<Value> {
        mirror::clear_wanted(&self.pg, LIKES_PLAYLISTS, sc_user_id, playlist_urn).await?;
        self.sync_queue
            .enqueue(sc_user_id, "unlike_playlist", playlist_urn, None)
            .await?;
        Ok(json!({ "status": "queued", "actionType": "unlike_playlist" }))
    }

    /// Холодная проверка лайка плейлиста: смотрим только в user_likes_playlists.
    /// Лайки, поставленные на SC web и ещё не утянутые refresh'ем, сюда не
    /// попадут — это ожидаемо (refresh их подтянет на следующем тике TTL).
    pub async fn is_playlist_liked(
        &self,
        sc_user_id: &str,
        playlist_urn: &str,
    ) -> AppResult<Value> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM user_likes_playlists \
             WHERE user_id = $1 AND playlist_urn = $2 AND wanted_state = true)",
        )
        .bind(sc_user_id)
        .bind(playlist_urn)
        .fetch_one(&self.pg)
        .await?;
        Ok(json!({ "liked": exists }))
    }
}
