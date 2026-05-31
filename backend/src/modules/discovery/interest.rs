use std::time::Duration;

use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::error::AppResult;

/// Periodically denormalize 30-day listen activity into artists.interest_score
/// (so the catalog claim is an index scan, not a per-tick user_events join), and
/// pull never-crawled active artists to due-now so user-relevant catalogs are
/// covered first in the initial sweep.
pub fn spawn(pg: PgPool, interval_sec: u64, shutdown: CancellationToken) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(Duration::from_secs(interval_sec.max(60)));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = shutdown.cancelled() => break,
                _ = ticker.tick() => {
                    if let Err(e) = recompute(&pg).await {
                        warn!(error = %e, "interest recompute failed");
                    }
                }
            }
        }
    });
}

async fn recompute(pg: &PgPool) -> AppResult<()> {
    let updated = sqlx::query(
        "WITH agg AS (
             SELECT ta.artist_id, COUNT(*)::real AS score
             FROM user_events ue
             JOIN tracks it ON it.sc_track_id = ue.sc_track_id
             JOIN track_artists ta ON ta.track_id = it.id
             WHERE ue.created_at > now() - interval '30 days'
             GROUP BY ta.artist_id
         )
         UPDATE artists a
         SET interest_score = agg.score
         FROM agg
         WHERE a.id = agg.artist_id AND a.interest_score IS DISTINCT FROM agg.score",
    )
        .execute(pg)
        .await?;

    // Surface active-but-never-crawled artists immediately (one-shot per artist:
    // gated on *_crawled_at IS NULL so already-crawled popular artists keep the
    // normal cadence and are not re-crawled every tick).
    sqlx::query(
        "UPDATE artists SET genius_next_run_at = now()
         WHERE interest_score > 0 AND merged_into IS NULL AND NOT crawl_dead
           AND genius_artist_id IS NOT NULL AND genius_crawled_at IS NULL
           AND genius_next_run_at > now()",
    )
        .execute(pg)
        .await?;

    debug!(rows = updated.rows_affected(), "interest recomputed");
    Ok(())
}
