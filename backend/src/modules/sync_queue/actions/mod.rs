use serde_json::Value;
use sqlx::PgPool;

use crate::error::{AppError, AppResult};
use crate::sc::ScClient;

pub mod comment;
pub mod follow_user;
pub mod like_playlist;
pub mod like_track;
pub mod playlist_create;
pub mod playlist_delete;
pub mod playlist_update;
pub mod unfollow_user;
pub mod unlike_playlist;
pub mod unlike_track;

/// Контекст выполнения одного action из sync_queue. Прокидывается во все
/// action-handler'ы, чтобы каждый сам мог обновить свой user_<state>-mirror
/// после успешного SC-вызова.
pub struct ActionCtx<'a> {
    pub sc: &'a ScClient,
    pub pg: &'a PgPool,
    pub token: &'a str,
    pub user_id: &'a str,
    pub target_urn: &'a str,
    pub payload: Option<&'a Value>,
}

/// Диспатч по action_type. Каждый action — отдельный модуль в `actions/`.
pub async fn dispatch(ctx: &ActionCtx<'_>, action_type: &str) -> AppResult<()> {
    match action_type {
        like_track::KIND => like_track::execute(ctx).await,
        unlike_track::KIND => unlike_track::execute(ctx).await,
        like_playlist::KIND => like_playlist::execute(ctx).await,
        unlike_playlist::KIND => unlike_playlist::execute(ctx).await,
        follow_user::KIND => follow_user::execute(ctx).await,
        unfollow_user::KIND => unfollow_user::execute(ctx).await,
        playlist_create::KIND => playlist_create::execute(ctx).await,
        playlist_update::KIND => playlist_update::execute(ctx).await,
        playlist_delete::KIND => playlist_delete::execute(ctx).await,
        comment::KIND => comment::execute(ctx).await,
        other => Err(AppError::bad_request(format!(
            "unknown sync_queue action_type: {other}"
        ))),
    }
}

/// Возвращает action_type противоположного действия (для дедупа на enqueue).
pub fn inverse(action_type: &str) -> Option<&'static str> {
    match action_type {
        like_track::KIND => Some(unlike_track::KIND),
        unlike_track::KIND => Some(like_track::KIND),
        like_playlist::KIND => Some(unlike_playlist::KIND),
        unlike_playlist::KIND => Some(like_playlist::KIND),
        follow_user::KIND => Some(unfollow_user::KIND),
        unfollow_user::KIND => Some(follow_user::KIND),
        _ => None,
    }
}
