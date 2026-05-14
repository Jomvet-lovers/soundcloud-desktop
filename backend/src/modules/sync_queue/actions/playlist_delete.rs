use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "playlist_delete";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/playlists/{}", ctx.target_urn), ctx.token)
        .await?;
    // Сервис уже удалил user_owned_playlists + cached_playlists в момент
    // запроса; добиваем на случай race с другим юзером/refresh'ем.
    sqlx::query("DELETE FROM cached_playlists WHERE playlist_urn = $1")
        .bind(ctx.target_urn)
        .execute(ctx.pg)
        .await?;
    sqlx::query("DELETE FROM cached_playlist_tracks WHERE playlist_urn = $1")
        .bind(ctx.target_urn)
        .execute(ctx.pg)
        .await?;
    sqlx::query("DELETE FROM user_owned_playlists WHERE user_id = $1 AND playlist_urn = $2")
        .bind(ctx.user_id)
        .bind(ctx.target_urn)
        .execute(ctx.pg)
        .await?;
    Ok(())
}
