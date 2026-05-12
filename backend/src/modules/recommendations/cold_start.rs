use sqlx::PgPool;

use crate::error::AppResult;

use super::service::RecommendationsService;

const FRESH_DAYS: i64 = 14;
const POOL_FRESH: i64 = 80;
const POOL_POPULAR: i64 = 80;

impl RecommendationsService {
    pub async fn cold_start_pool(
        &self,
        languages: Option<&[String]>,
        limit: usize,
    ) -> AppResult<Vec<String>> {
        let lang_filter: Option<Vec<String>> = languages.map(|v| v.to_vec());

        let (fresh, popular) =
            tokio::join!(load_fresh(&self.pg, lang_filter.as_deref()), load_popular(&self.pg, lang_filter.as_deref()));
        let fresh = fresh.unwrap_or_default();
        let popular = popular.unwrap_or_default();

        let mut combined: Vec<String> = Vec::with_capacity(fresh.len() + popular.len());
        let mut seen = std::collections::HashSet::new();
        let mut fi = 0;
        let mut pi = 0;
        while combined.len() < limit * 4 && (fi < fresh.len() || pi < popular.len()) {
            if fi < fresh.len() {
                let id = &fresh[fi];
                if seen.insert(id.clone()) {
                    combined.push(id.clone());
                }
                fi += 1;
            }
            if pi < popular.len() {
                let id = &popular[pi];
                if seen.insert(id.clone()) {
                    combined.push(id.clone());
                }
                pi += 1;
            }
        }
        Ok(combined)
    }
}

async fn load_fresh(pg: &PgPool, languages: Option<&[String]>) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = if let Some(langs) = languages {
        if !langs.is_empty() {
            sqlx::query_as(
                "SELECT sc_track_id FROM indexed_tracks
                 WHERE indexed_at IS NOT NULL
                   AND indexed_at > NOW() - make_interval(days => $2::int)
                   AND language = ANY($1)
                 ORDER BY indexed_at DESC
                 LIMIT $3",
            )
            .bind(langs)
            .bind(FRESH_DAYS)
            .bind(POOL_FRESH)
            .fetch_all(pg)
            .await?
        } else {
            sqlx::query_as(
                "SELECT sc_track_id FROM indexed_tracks
                 WHERE indexed_at IS NOT NULL
                   AND indexed_at > NOW() - make_interval(days => $1::int)
                 ORDER BY indexed_at DESC
                 LIMIT $2",
            )
            .bind(FRESH_DAYS)
            .bind(POOL_FRESH)
            .fetch_all(pg)
            .await?
        }
    } else {
        sqlx::query_as(
            "SELECT sc_track_id FROM indexed_tracks
             WHERE indexed_at IS NOT NULL
               AND indexed_at > NOW() - make_interval(days => $1::int)
             ORDER BY indexed_at DESC
             LIMIT $2",
        )
        .bind(FRESH_DAYS)
        .bind(POOL_FRESH)
        .fetch_all(pg)
        .await?
    };
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn load_popular(pg: &PgPool, languages: Option<&[String]>) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = if let Some(langs) = languages {
        if !langs.is_empty() {
            sqlx::query_as(
                "SELECT it.sc_track_id
                 FROM indexed_tracks it
                 JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                 WHERE it.indexed_at IS NOT NULL AND it.language = ANY($1)
                 ORDER BY COALESCE(c.play_count, 0) DESC
                 LIMIT $2",
            )
            .bind(langs)
            .bind(POOL_POPULAR)
            .fetch_all(pg)
            .await?
        } else {
            sqlx::query_as(
                "SELECT it.sc_track_id
                 FROM indexed_tracks it
                 JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                 WHERE it.indexed_at IS NOT NULL
                 ORDER BY COALESCE(c.play_count, 0) DESC
                 LIMIT $1",
            )
            .bind(POOL_POPULAR)
            .fetch_all(pg)
            .await?
        }
    } else {
        sqlx::query_as(
            "SELECT it.sc_track_id
             FROM indexed_tracks it
             JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
             WHERE it.indexed_at IS NOT NULL
             ORDER BY COALESCE(c.play_count, 0) DESC
             LIMIT $1",
        )
        .bind(POOL_POPULAR)
        .fetch_all(pg)
        .await?
    };
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
