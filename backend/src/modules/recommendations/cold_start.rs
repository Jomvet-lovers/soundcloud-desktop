use sqlx::PgPool;

use crate::error::AppResult;

use super::service::RecommendationsService;

const FRESH_DAYS: i32 = 14;
const POOL_FRESH: i64 = 80;
const POOL_POPULAR: i64 = 80;

impl RecommendationsService {
    pub async fn cold_start_pool(
        &self,
        languages: Option<&[String]>,
        limit: usize,
    ) -> AppResult<Vec<String>> {
        let lang_filter: Option<Vec<String>> = languages.map(|v| v.to_vec());

        let (fresh, popular) = tokio::join!(
            load_fresh(&self.pg, lang_filter.as_deref()),
            load_popular(&self.pg, lang_filter.as_deref())
        );
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
    let rows: Vec<String> = if let Some(langs) = languages {
        if !langs.is_empty() {
            sqlx::query_file_scalar!(
                "queries/recommendations/cold_start/fresh_lang.sql",
                langs,
                FRESH_DAYS,
                POOL_FRESH
            )
            .fetch_all(pg)
            .await?
        } else {
            sqlx::query_file_scalar!(
                "queries/recommendations/cold_start/fresh_nolang.sql",
                FRESH_DAYS,
                POOL_FRESH
            )
            .fetch_all(pg)
            .await?
        }
    } else {
        sqlx::query_file_scalar!(
            "queries/recommendations/cold_start/fresh_nolang.sql",
            FRESH_DAYS,
            POOL_FRESH
        )
        .fetch_all(pg)
        .await?
    };
    Ok(rows)
}

async fn load_popular(pg: &PgPool, languages: Option<&[String]>) -> AppResult<Vec<String>> {
    let rows: Vec<String> = if let Some(langs) = languages {
        if !langs.is_empty() {
            sqlx::query_file_scalar!(
                "queries/recommendations/cold_start/popular_lang.sql",
                langs,
                POOL_POPULAR
            )
            .fetch_all(pg)
            .await?
        } else {
            sqlx::query_file_scalar!(
                "queries/recommendations/cold_start/popular_nolang.sql",
                POOL_POPULAR
            )
            .fetch_all(pg)
            .await?
        }
    } else {
        sqlx::query_file_scalar!(
            "queries/recommendations/cold_start/popular_nolang.sql",
            POOL_POPULAR
        )
        .fetch_all(pg)
        .await?
    };
    Ok(rows)
}
