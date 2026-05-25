use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use futures::stream::{self, StreamExt};
use reqwest::Client;
use sqlx::PgPool;
use tracing::debug;

use crate::error::AppResult;

const MISS_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const HEAD_CONCURRENCY: usize = 16;
const HEAD_TIMEOUT: Duration = Duration::from_secs(3);

pub struct S3VerifierService {
    http: Client,
    storage_url: String,
    pg: PgPool,
}

impl S3VerifierService {
    pub fn new(http: Client, storage_url: String, pg: PgPool) -> Arc<Self> {
        let trimmed = storage_url.trim_end_matches('/').to_string();
        Arc::new(Self {
            http,
            storage_url: trimmed,
            pg,
        })
    }

    pub async fn find_missing(&self, sc_track_ids: &[String]) -> AppResult<HashSet<String>> {
        let mut missing: HashSet<String> = HashSet::new();
        if sc_track_ids.is_empty() || self.storage_url.is_empty() {
            return Ok(missing);
        }

        let ttl_cutoff = Utc::now() - chrono::Duration::from_std(MISS_TTL).unwrap();

        let rows: Vec<(String, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> = sqlx::query_as(
            "SELECT sc_track_id, s3_verified_at, s3_missing_at FROM tracks \
             WHERE sc_track_id = ANY($1)",
        )
        .bind(sc_track_ids)
        .fetch_all(&self.pg)
        .await?;
        let mut by_id: std::collections::HashMap<
            String,
            (Option<DateTime<Utc>>, Option<DateTime<Utc>>),
        > = std::collections::HashMap::new();
        for (id, v, m) in rows {
            by_id.insert(id, (v, m));
        }

        let mut to_check: Vec<String> = Vec::new();
        for id in sc_track_ids {
            match by_id.get(id) {
                Some((Some(verified), m)) if m.map(|x| x <= *verified).unwrap_or(true) => {
                    continue;
                }
                Some((_, Some(miss))) if *miss > ttl_cutoff => {
                    missing.insert(id.clone());
                }
                _ => to_check.push(id.clone()),
            }
        }
        if to_check.is_empty() {
            return Ok(missing);
        }

        let checks = stream::iter(to_check.iter().cloned())
            .map(|id| {
                let this = self;
                async move {
                    let found = this.probe(&id).await;
                    (id, found)
                }
            })
            .buffer_unordered(HEAD_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;

        let mut ok_ids: Vec<String> = Vec::new();
        let mut miss_ids: Vec<String> = Vec::new();
        for (id, found) in checks {
            if found {
                ok_ids.push(id);
            } else {
                missing.insert(id.clone());
                miss_ids.push(id);
            }
        }

        if !ok_ids.is_empty() {
            sqlx::query(
                "UPDATE tracks SET \
                     storage_state = 'ok', \
                     s3_verified_at = now(), \
                     s3_missing_at = NULL \
                 WHERE sc_track_id = ANY($1)",
            )
            .bind(&ok_ids)
            .execute(&self.pg)
            .await?;
        }
        if !miss_ids.is_empty() {
            sqlx::query(
                "UPDATE tracks SET \
                     storage_state = CASE \
                         WHEN storage_state = 'pending' THEN 'pending' \
                         ELSE 'missing' \
                     END, \
                     s3_missing_at = now() \
                 WHERE sc_track_id = ANY($1)",
            )
            .bind(&miss_ids)
            .execute(&self.pg)
            .await?;
            debug!(
                misses = miss_ids.len(),
                oks = ok_ids.len(),
                "S3 verify result"
            );
        }
        Ok(missing)
    }

    async fn probe(&self, sc_track_id: &str) -> bool {
        let url = format!("{}/soundcloud_tracks_{sc_track_id}.m4a", self.storage_url);
        match self.http.head(&url).timeout(HEAD_TIMEOUT).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    return true;
                }
                if status != 404 && status != 410 {
                    debug!(url, status, "HEAD non-404");
                }
                false
            }
            Err(e) => {
                debug!(url, error = %e, "HEAD failed");
                false
            }
        }
    }
}
