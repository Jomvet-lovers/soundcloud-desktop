use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "comment";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_post_value(
            &format!("/tracks/{}/comments", ctx.target_urn),
            ctx.token,
            ctx.payload,
        )
        .await?;
    Ok(())
}
