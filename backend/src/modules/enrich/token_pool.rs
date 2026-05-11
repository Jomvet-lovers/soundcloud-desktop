use sqlx::PgPool;

use crate::error::AppResult;

pub struct TokenPool {
    pg: PgPool,
}

impl TokenPool {
    pub fn new(pg: PgPool) -> Self {
        Self { pg }
    }

    pub async fn pick_for_background(&self, extra_random: usize) -> AppResult<Vec<String>> {
        let latest: Option<(String,)> = sqlx::query_as(
            "SELECT access_token FROM sessions
             WHERE access_token <> '' AND expires_at > now() + interval '1 minute'
             ORDER BY updated_at DESC NULLS LAST
             LIMIT 1",
        )
        .fetch_optional(&self.pg)
        .await?;

        let mut out: Vec<String> = Vec::new();
        if let Some((t,)) = latest {
            out.push(t);
        }
        if extra_random > 0 {
            let exclude = out.first().cloned().unwrap_or_default();
            let extras: Vec<(String,)> = sqlx::query_as(
                "SELECT access_token FROM sessions
                 WHERE access_token <> ''
                   AND access_token <> $1
                   AND expires_at > now() + interval '1 minute'
                 ORDER BY random()
                 LIMIT $2",
            )
            .bind(&exclude)
            .bind(extra_random as i64)
            .fetch_all(&self.pg)
            .await?;
            out.extend(extras.into_iter().map(|(t,)| t));
        }
        Ok(out)
    }
}
