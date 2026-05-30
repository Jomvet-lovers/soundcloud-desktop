use serde_json::json;

use crate::common::sc_ids::extract_sc_id;
use crate::error::{AppError, AppResult};

use super::ActionCtx;

pub const KIND: &str = "track_sharing";

/// Write-back смены приватности трека в SC. Локальную `tracks.sharing` уже
/// обновил optimistic-путь в сервисе; здесь подтверждаем после SC-ack
/// (reconcile на случай гонки с cold-refresh'ем).
pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    let sharing = ctx
        .payload
        .and_then(|p| p.get("sharing"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::bad_request("track_sharing: missing sharing"))?;
    let body = json!({ "track": { "sharing": sharing } });
    ctx.sc
        .api_put_value(
            &format!("/tracks/{}", ctx.target_urn),
            ctx.token,
            Some(&body),
        )
        .await?;
    sqlx::query("UPDATE tracks SET sharing = $2, updated_at = now() WHERE sc_track_id = $1")
        .bind(extract_sc_id(ctx.target_urn))
        .bind(sharing)
        .execute(ctx.pg)
        .await?;
    Ok(())
}
