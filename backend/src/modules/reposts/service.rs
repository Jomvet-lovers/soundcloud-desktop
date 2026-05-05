use std::sync::Arc;

use serde_json::{json, Value};

use crate::error::AppResult;
use crate::modules::pending_actions::PendingActionsService;
use crate::sc::ScClient;

pub struct RepostsService {
    sc: ScClient,
    pending: Arc<PendingActionsService>,
}

impl RepostsService {
    pub fn new(sc: ScClient, pending: Arc<PendingActionsService>) -> Arc<Self> {
        Arc::new(Self { sc, pending })
    }

    pub async fn repost_track(
        &self,
        token: &str,
        session_id: &str,
        track_urn: &str,
    ) -> AppResult<Value> {
        match self
            .sc
            .api_post::<Value, Value>(&format!("/reposts/tracks/{track_urn}"), token, None)
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending.enqueue(session_id, "repost", track_urn, None).await?;
                Ok(json!({ "queued": true, "actionType": "repost", "targetUrn": track_urn }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn remove_track_repost(
        &self,
        token: &str,
        session_id: &str,
        track_urn: &str,
    ) -> AppResult<Value> {
        match self.sc.api_delete(&format!("/reposts/tracks/{track_urn}"), token).await {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending.enqueue(session_id, "unrepost", track_urn, None).await?;
                Ok(json!({ "queued": true, "actionType": "unrepost", "targetUrn": track_urn }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn repost_playlist(
        &self,
        token: &str,
        session_id: &str,
        playlist_urn: &str,
    ) -> AppResult<Value> {
        match self
            .sc
            .api_post::<Value, Value>(&format!("/reposts/playlists/{playlist_urn}"), token, None)
            .await
        {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "repost_playlist", playlist_urn, None)
                    .await?;
                Ok(json!({
                    "queued": true,
                    "actionType": "repost_playlist",
                    "targetUrn": playlist_urn,
                }))
            }
            Err(e) => Err(e),
        }
    }

    pub async fn remove_playlist_repost(
        &self,
        token: &str,
        session_id: &str,
        playlist_urn: &str,
    ) -> AppResult<Value> {
        match self.sc.api_delete(&format!("/reposts/playlists/{playlist_urn}"), token).await {
            Ok(v) => Ok(v),
            Err(e) if PendingActionsService::is_ban_error(&e) => {
                self.pending
                    .enqueue(session_id, "unrepost_playlist", playlist_urn, None)
                    .await?;
                Ok(json!({
                    "queued": true,
                    "actionType": "unrepost_playlist",
                    "targetUrn": playlist_urn,
                }))
            }
            Err(e) => Err(e),
        }
    }
}
