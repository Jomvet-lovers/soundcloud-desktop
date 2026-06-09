use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountRole {
    Main,
    Demo,
    Alt,
}

impl AccountRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Demo => "demo",
            Self::Alt => "alt",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "main" => Some(Self::Main),
            "demo" => Some(Self::Demo),
            "alt" => Some(Self::Alt),
            _ => None,
        }
    }
}

pub async fn upsert(
    pg: &PgPool,
    artist_id: Uuid,
    sc_user_id: &str,
    role: AccountRole,
    source: &str,
    verified: bool,
) -> AppResult<()> {
    if sc_user_id.is_empty() {
        return Ok(());
    }
    sqlx::query_file!(
        "queries/enrich/sc_accounts/upsert_account.sql",
        artist_id,
        sc_user_id,
        role.as_str(),
        source,
        verified
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/enrich/sc_accounts/backfill_artist_sc_user_id.sql",
        artist_id,
        sc_user_id
    )
    .execute(pg)
    .await?;
    Ok(())
}

pub async fn delete(pg: &PgPool, artist_id: Uuid, sc_user_id: &str) -> AppResult<bool> {
    let res = sqlx::query_file!(
        "queries/enrich/sc_accounts/delete_account.sql",
        artist_id,
        sc_user_id
    )
    .execute(pg)
    .await?;
    Ok(res.rows_affected() > 0)
}

pub fn extract_sc_user_id_from_resolve(value: &serde_json::Value) -> Option<String> {
    if let Some(kind) = value.get("kind").and_then(|v| v.as_str()) {
        if kind != "user" {
            return None;
        }
    }
    if let Some(urn) = value.get("urn").and_then(|v| v.as_str()) {
        if let Some(id) = urn.rsplit(':').next() {
            if !id.is_empty() && id.bytes().all(|b| b.is_ascii_digit()) {
                return Some(id.to_string());
            }
        }
    }
    if let Some(id) = value.get("id").and_then(|v| v.as_i64()) {
        return Some(id.to_string());
    }
    None
}

pub fn is_soundcloud_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if let Ok(parsed) = url::Url::parse(&lower) {
        if let Some(host) = parsed.host_str() {
            let h = host.strip_prefix("www.").unwrap_or(host);
            if h == "soundcloud.com" || h == "m.soundcloud.com" {
                let path = parsed.path().trim_start_matches('/');
                let first = path.split('/').next().unwrap_or("");
                return !first.is_empty()
                    && !matches!(
                        first,
                        "discover"
                            | "search"
                            | "you"
                            | "stream"
                            | "feed"
                            | "messages"
                            | "settings"
                            | "tags"
                            | "stations"
                            | "embed"
                    );
            }
        }
    }
    false
}
