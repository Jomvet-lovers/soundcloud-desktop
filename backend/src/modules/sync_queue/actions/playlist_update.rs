use crate::error::AppResult;

use super::ActionCtx;

pub const KIND: &str = "playlist_update";

pub async fn execute(ctx: &ActionCtx<'_>) -> AppResult<()> {
    ctx.sc
        .api_put_value(
            &format!("/playlists/{}", ctx.target_urn),
            ctx.token,
            ctx.payload,
        )
        .await?;
    // Инвалидируем нормализованную playlists-строку: следующее чтение
    // принесёт свежие данные из SC. Делаем после SC-ack — до этого момента
    // читать стейл OK.
    sqlx::query("DELETE FROM playlists WHERE urn = $1")
        .bind(ctx.target_urn)
        .execute(ctx.pg)
        .await?;
    sqlx::query("DELETE FROM playlist_tracks WHERE playlist_urn = $1")
        .bind(ctx.target_urn)
        .execute(ctx.pg)
        .await?;
    Ok(())
}
