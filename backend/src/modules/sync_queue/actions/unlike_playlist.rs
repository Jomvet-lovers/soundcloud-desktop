use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "unlike_playlist";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/likes/playlists/{}", ctx.target_urn), ctx.token)
        .await?;
    sqlx::query(
        "DELETE FROM user_likes_playlists \
         WHERE user_id = $1 AND playlist_urn = $2 AND wanted_state = false",
    )
    .bind(ctx.user_id)
    .bind(ctx.target_urn)
    .execute(ctx.pg)
    .await?;
    Ok(())
}
