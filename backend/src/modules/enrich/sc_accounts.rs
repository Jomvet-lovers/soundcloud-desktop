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
    sqlx::query(
        "INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified)
         VALUES ($1, $2, $3, $4, $5)
         ON CONFLICT (artist_id, sc_user_id) DO UPDATE
            SET role     = CASE WHEN EXCLUDED.verified AND NOT artist_sc_accounts.verified
                                THEN EXCLUDED.role ELSE artist_sc_accounts.role END,
                verified = artist_sc_accounts.verified OR EXCLUDED.verified,
                source   = CASE WHEN artist_sc_accounts.source = 'manual'
                                THEN artist_sc_accounts.source ELSE EXCLUDED.source END,
                updated_at = now()",
    )
    .bind(artist_id)
    .bind(sc_user_id)
    .bind(role.as_str())
    .bind(source)
    .bind(verified)
    .execute(pg)
    .await?;
    sqlx::query(
        "UPDATE artists SET sc_user_id = COALESCE(sc_user_id, $2), updated_at = now()
         WHERE id = $1",
    )
    .bind(artist_id)
    .bind(sc_user_id)
    .execute(pg)
    .await?;
    Ok(())
}

pub async fn delete(pg: &PgPool, artist_id: Uuid, sc_user_id: &str) -> AppResult<bool> {
    let res = sqlx::query(
        "DELETE FROM artist_sc_accounts WHERE artist_id = $1 AND sc_user_id = $2",
    )
    .bind(artist_id)
    .bind(sc_user_id)
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
