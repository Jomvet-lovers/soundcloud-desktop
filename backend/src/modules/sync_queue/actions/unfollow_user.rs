use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "unfollow_user";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/me/followings/{}", ctx.target_urn), ctx.token)
        .await?;
    sqlx::query_file!(
        "queries/sync_queue/actions/unfollow_user/delete.sql",
        ctx.user_id,
        ctx.target_urn
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
