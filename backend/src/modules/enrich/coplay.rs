use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;

pub async fn recompute_for_track(pg: &PgPool, track_id: Uuid) -> AppResult<()> {
    sqlx::query_file!("queries/enrich/coplay/recompute_for_track.sql", track_id)
        .execute(pg)
        .await?;
    Ok(())
}
