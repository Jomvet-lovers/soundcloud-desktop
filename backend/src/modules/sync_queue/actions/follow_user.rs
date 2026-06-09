use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "follow_user";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_put_value(
            &format!("/me/followings/{}", ctx.target_urn),
            ctx.token,
            None,
        )
        .await?;
    sqlx::query_file!(
        "queries/sync_queue/actions/follow_user/update_synced.sql",
        ctx.user_id,
        ctx.target_urn,
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
