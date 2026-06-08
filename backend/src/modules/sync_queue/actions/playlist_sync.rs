use serde_json::{json, Value};

use crate::error::AppResult;
use crate::modules::playlists::PlaylistRepository;

use super::ActionCtx;

pub const KIND: &str = "playlist_sync";

/// Пушит ТЕКУЩИЙ desired-state плейлиста (читается из нашей БД в рантайме, не из
/// устаревшего payload) полным списком в SC. На успехе фиксирует synced_rev =
/// pushed_rev ТОЛЬКО если desired_rev не ушёл вперёд под нами. Локальные
/// playlist_tracks НИКОГДА не удаляет (наша БД — источник истины). Если правка
/// прилетела между read и SC-ack — её enqueue сбросил locked_at, optimistic
/// delete воркера промахнётся, строка переотправится следующим тиком (PUT
/// идемпотентен).
pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    let repo = PlaylistRepository::new(ctx.pg.clone());
    let Some((rev, ids)) = repo.desired_snapshot(ctx.target_urn).await? else {
        return Ok(());
    };

    let tracks: Vec<Value> = ids
        .iter()
        .map(|id| match id.parse::<i64>() {
            Ok(n) => json!({ "id": n }),
            Err(_) => json!({ "id": id }),
        })
        .collect();
    let body = json!({ "playlist": { "tracks": tracks } });

    ctx.sc
        .api_put_value(
            &format!("/playlists/{}", ctx.target_urn),
            ctx.token,
            Some(&body),
        )
        .await?;

    let _ = repo.mark_synced_if_unchanged(ctx.target_urn, rev).await?;
    Ok(())
}
