use serde_json::json;

use crate::error::{AppError, AppResult};

use super::ActionCtx;

pub const KIND: &str = "playlist_sharing";

/// Write-back смены приватности плейлиста в SC + reconcile нашей
/// `playlists.sharing`. В отличие от `playlist_update` строку НЕ удаляем —
/// меняется один флаг, инвалидировать весь плейлист незачем (иначе у владельца
/// он мигнёт «пропал → перезагрузился»).
pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    let sharing = ctx
        .payload
        .and_then(|p| p.get("sharing"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AppError::bad_request("playlist_sharing: missing sharing"))?;
    let body = json!({ "playlist": { "sharing": sharing } });
    ctx.sc
        .api_put_value(
            &format!("/playlists/{}", ctx.target_urn),
            ctx.token,
            Some(&body),
        )
        .await?;
    sqlx::query_file!(
        "queries/sync_queue/actions/playlist_sharing/update_sharing.sql",
        ctx.target_urn,
        sharing
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
