use std::collections::HashMap;

use rand::distributions::Distribution;
use rand_distr::Beta;
use sqlx::PgPool;

use crate::error::AppResult;

const PRIOR_ALPHA: f64 = 1.0;
const PRIOR_BETA: f64 = 1.0;
const CLICK_WEIGHT: f64 = 0.4;
const COMPLETE_WEIGHT: f64 = 0.6;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ClusterStat {
    pub cluster_id: String,
    pub shows: i64,
    pub clicks: i64,
    pub completes: i64,
}

pub async fn load_stats(pg: &PgPool, sc_user_id: &str) -> AppResult<HashMap<String, ClusterStat>> {
    let rows: Vec<ClusterStat> = sqlx::query_as(
        "SELECT cluster_id, shows, clicks, completes
         FROM cluster_bandit_stats
         WHERE sc_user_id = $1",
    )
    .bind(sc_user_id)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|r| (r.cluster_id.clone(), r)).collect())
}

pub fn sample_priorities(stats: &HashMap<String, ClusterStat>, clusters: &[&str]) -> Vec<f64> {
    let mut rng = rand::thread_rng();
    clusters
        .iter()
        .map(|c| {
            let stat = stats.get(*c);
            let (alpha, beta) = match stat {
                Some(s) => {
                    let positive =
                        (s.clicks as f64) * CLICK_WEIGHT + (s.completes as f64) * COMPLETE_WEIGHT;
                    let negative = (s.shows as f64 - positive).max(0.0);
                    (PRIOR_ALPHA + positive, PRIOR_BETA + negative)
                }
                None => (PRIOR_ALPHA, PRIOR_BETA),
            };
            match Beta::new(alpha, beta) {
                Ok(d) => d.sample(&mut rng),
                Err(_) => 0.5,
            }
        })
        .collect()
}

pub fn order_by_thompson<'a>(
    clusters: &'a [&'a str],
    stats: &HashMap<String, ClusterStat>,
) -> Vec<&'a str> {
    let priorities = sample_priorities(stats, clusters);
    let mut indexed: Vec<(usize, f64)> = priorities.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    indexed.into_iter().map(|(i, _)| clusters[i]).collect()
}

pub async fn record_shows(
    pg: &PgPool,
    sc_user_id: &str,
    counts: &[(String, i64)],
) -> AppResult<()> {
    for (cluster, n) in counts {
        if *n <= 0 {
            continue;
        }
        sqlx::query(
            "INSERT INTO cluster_bandit_stats (sc_user_id, cluster_id, shows, updated_at)
             VALUES ($1, $2, $3, NOW())
             ON CONFLICT (sc_user_id, cluster_id)
             DO UPDATE SET shows = cluster_bandit_stats.shows + EXCLUDED.shows,
                           updated_at = NOW()",
        )
        .bind(sc_user_id)
        .bind(cluster)
        .bind(*n)
        .execute(pg)
        .await?;
    }
    Ok(())
}

pub async fn record_outcome(
    pg: &PgPool,
    sc_user_id: &str,
    cluster_id: &str,
    clicks: i64,
    completes: i64,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO cluster_bandit_stats (sc_user_id, cluster_id, clicks, completes, updated_at)
         VALUES ($1, $2, $3, $4, NOW())
         ON CONFLICT (sc_user_id, cluster_id)
         DO UPDATE SET clicks = cluster_bandit_stats.clicks + EXCLUDED.clicks,
                       completes = cluster_bandit_stats.completes + EXCLUDED.completes,
                       updated_at = NOW()",
    )
    .bind(sc_user_id)
    .bind(cluster_id)
    .bind(clicks)
    .bind(completes)
    .execute(pg)
    .await?;
    Ok(())
}
