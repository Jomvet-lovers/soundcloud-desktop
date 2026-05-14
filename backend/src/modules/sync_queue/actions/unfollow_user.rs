use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "unfollow_user";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/me/followings/{}", ctx.target_urn), ctx.token)
        .await?;
    sqlx::query(
        "DELETE FROM user_followings \
         WHERE user_id = $1 AND target_user_urn = $2 AND wanted_state = false",
    )
    .bind(ctx.user_id)
    .bind(ctx.target_urn)
    .execute(ctx.pg)
    .await?;
    Ok(())
}
