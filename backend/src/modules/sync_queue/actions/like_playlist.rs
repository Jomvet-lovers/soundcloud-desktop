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
    sqlx::query_file!(
        "queries/sync_queue/actions/like_playlist/update_synced.sql",
        ctx.user_id,
        ctx.target_urn,
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
