use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "unlike_playlist";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/likes/playlists/{}", ctx.target_urn), ctx.token)
        .await?;
    sqlx::query_file!(
        "queries/sync_queue/actions/unlike_playlist/delete_like.sql",
        ctx.user_id,
        ctx.target_urn,
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
