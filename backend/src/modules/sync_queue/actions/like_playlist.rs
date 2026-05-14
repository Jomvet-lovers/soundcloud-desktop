use serde_json::Value;

use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "like_playlist";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_post::<Value, Value>(
            &format!("/likes/playlists/{}", ctx.target_urn),
            ctx.token,
            None,
        )
        .await?;
    sqlx::query(
        "UPDATE user_likes_playlists \
         SET progress = false, synced_at = now() \
         WHERE user_id = $1 AND playlist_urn = $2 AND wanted_state = true",
    )
    .bind(ctx.user_id)
    .bind(ctx.target_urn)
    .execute(ctx.pg)
    .await?;
    Ok(())
}
