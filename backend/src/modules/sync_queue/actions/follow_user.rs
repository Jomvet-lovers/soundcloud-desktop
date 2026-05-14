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
    sqlx::query(
        "UPDATE user_followings \
         SET progress = false, synced_at = now() \
         WHERE user_id = $1 AND target_user_urn = $2 AND wanted_state = true",
    )
    .bind(ctx.user_id)
    .bind(ctx.target_urn)
    .execute(ctx.pg)
    .await?;
    Ok(())
}
