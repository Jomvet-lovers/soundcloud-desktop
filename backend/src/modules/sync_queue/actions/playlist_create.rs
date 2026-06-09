use serde_json::Value;

use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "playlist_create";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    let created: Value = ctx
        .sc
        .api_post_value("/playlists", ctx.token, ctx.payload)
        .await?;
    let Some(urn) = created.get("urn").and_then(|v| v.as_str()) else {
        return Ok(());
    };
    let is_public = created.get("sharing").and_then(|v| v.as_str()) == Some("public");

    // Приватный subset идёт в собственное зеркало юзера, чтобы /me/playlists
    // сразу вернул новый плейлист со всеми приватными полями. В shared
    // `playlists` зеркалируем только public-копию — её увидят все.
    if is_public {
        let repo = crate::modules::playlists::PlaylistRepository::new(ctx.pg.clone());
        let _ = repo.upsert_from_sc(&created).await;
    }
    // Миграция 0019 дропнула колонку `payload`; зеркало владения просто фиксирует
    // (user_id, playlist_urn) — приватные поля уже легли в playlists через upsert.
    sqlx::query_file!(
        "queries/sync_queue/actions/playlist_create/upsert_owned.sql",
        ctx.user_id,
        urn
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
