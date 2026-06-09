use serde_json::Value;

use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "like_track";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_post::<Value, Value>(
            &format!("/likes/tracks/{}", ctx.target_urn),
            ctx.token,
            None,
        )
        .await?;
    let sc_track_id = extract_sc_id(ctx.target_urn);
    sqlx::query_file!(
        "queries/sync_queue/actions/like_track/clear_progress.sql",
        ctx.user_id,
        sc_track_id
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
