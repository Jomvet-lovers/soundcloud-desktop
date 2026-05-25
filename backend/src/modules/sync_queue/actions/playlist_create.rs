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
    sqlx::query(
        "INSERT INTO user_owned_playlists (user_id, playlist_urn, payload, progress, synced_at) \
         VALUES ($1, $2, $3, false, now()) \
         ON CONFLICT (user_id, playlist_urn) DO UPDATE SET \
             payload = EXCLUDED.payload, synced_at = now()",
    )
    .bind(ctx.user_id)
    .bind(urn)
    .bind(&created)
    .execute(ctx.pg)
    .await?;
    Ok(())
}
