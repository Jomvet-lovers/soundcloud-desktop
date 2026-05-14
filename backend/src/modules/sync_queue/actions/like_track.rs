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
    // wanted_state=true гарантирует, что мы не снимаем progress с строки,
    // которую юзер уже успел перевернуть в pending-unlike.
    sqlx::query(
        "UPDATE user_likes_tracks \
         SET progress = false, synced_at = now() \
         WHERE user_id = $1 AND sc_track_id = $2 AND wanted_state = true",
    )
    .bind(ctx.user_id)
    .bind(sc_track_id)
    .execute(ctx.pg)
    .await?;
    Ok(())
}
