use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "playlist_delete";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_delete(&format!("/playlists/{}", ctx.target_urn), ctx.token)
        .await?;
    // Сервис уже удалил user_owned_playlists + playlists в момент запроса;
    // добиваем на случай race с другим юзером/refresh'ем.
    sqlx::query_file!(
        "queries/sync_queue/actions/playlist_delete/delete_playlist.sql",
        ctx.target_urn
    )
    .execute(ctx.pg)
    .await?;
    sqlx::query_file!(
        "queries/sync_queue/actions/playlist_delete/delete_playlist_tracks.sql",
        ctx.target_urn
    )
    .execute(ctx.pg)
    .await?;
    let variants = crate::common::sc_ids::user_id_variants(ctx.user_id);
    sqlx::query_file!(
        "queries/sync_queue/actions/playlist_delete/delete_user_owned.sql",
        &variants,
        ctx.target_urn
    )
    .execute(ctx.pg)
    .await?;
    Ok(())
}
