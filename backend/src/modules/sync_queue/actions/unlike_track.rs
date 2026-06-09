use crate::common::sc_ids::extract_sc_id;
use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "unlike_track";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/likes/tracks/{}", ctx.target_urn), ctx.token)
        .await?;
    let sc_track_id = extract_sc_id(ctx.target_urn);
    // wanted_state=false → строка остаётся только если юзер всё ещё хочет
    // unlike. Если за время лока юзер перевернул обратно в liked
    // (wanted_state=true), мы её не трогаем.
    sqlx::query_file!(
        "queries/sync_queue/actions/unlike_track/delete_like.sql",
        ctx.user_id,
        sc_track_id,
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
