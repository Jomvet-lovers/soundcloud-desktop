//! Нормализованная сущность `users` (без raw payload). Read-path проецирует
//! обратно в SC-shape.

use chrono::{DateTime, Utc};
use serde_json::{json, Map, Value};
use sqlx::FromRow;
use sqlx::PgPool;

use crate::common::sc_payload::{parse_dt, parse_id_or_string, string_field};
use crate::error::AppResult;

#[derive(Debug, Clone, FromRow)]
#[allow(dead_code)]
pub struct UserRow {
    pub sc_user_id: String,
    pub urn: String,
    pub username: String,
    pub username_normalized: String,
    pub full_name: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub permalink: Option<String>,
    pub permalink_url: Option<String>,
    pub avatar_url: Option<String>,
    pub country: Option<String>,
    pub city: Option<String>,
    pub description: Option<String>,
    pub verified: bool,
    pub followers_count: Option<i64>,
    pub followings_count: Option<i64>,
    pub tracks_count: Option<i64>,
    pub playlists_count: Option<i64>,
    pub reposts_count: Option<i64>,
    pub comments_count: Option<i64>,
    pub kind: Option<String>,
    pub sc_created_at: Option<DateTime<Utc>>,
    pub sc_last_modified: Option<DateTime<Utc>>,
    pub sc_synced_at: DateTime<Utc>,
    pub last_read_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

pub struct UserRepository {
    pg: PgPool,
}

impl UserRepository {
    pub fn new(pg: PgPool) -> Self {
        Self { pg }
    }

    pub async fn find_by_urn(&self, urn: &str) -> AppResult<Option<UserRow>> {
        let row: Option<UserRow> = sqlx::query_as("SELECT * FROM users WHERE urn = $1")
            .bind(urn)
            .fetch_optional(&self.pg)
            .await?;
        Ok(row)
    }

    pub async fn touch_last_read(&self, urn: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE users SET last_read_at = now() \
             WHERE urn = $1 \
               AND (last_read_at IS NULL OR last_read_at < now() - INTERVAL '5 minutes')",
        )
        .bind(urn)
        .execute(&self.pg)
        .await?;
        Ok(())
    }

    /// UPSERT из SC payload. Возвращает true если строка только что создана.
    pub async fn upsert_from_sc(&self, payload: &Value) -> AppResult<bool> {
        let Some(fields) = ScUserFields::from_sc(payload) else {
            return Ok(false);
        };
        let row: (bool,) = sqlx::query_as(
            "INSERT INTO users (
                sc_user_id, urn, username, username_normalized, full_name, first_name, last_name,
                permalink, permalink_url, avatar_url, country, city, description, verified,
                followers_count, followings_count, tracks_count, playlists_count,
                reposts_count, comments_count, kind, sc_created_at, sc_last_modified, sc_synced_at
             ) VALUES (
                $1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,now()
             )
             ON CONFLICT (sc_user_id) DO UPDATE SET
                urn = EXCLUDED.urn,
                username = EXCLUDED.username,
                username_normalized = EXCLUDED.username_normalized,
                full_name = EXCLUDED.full_name,
                first_name = EXCLUDED.first_name,
                last_name = EXCLUDED.last_name,
                permalink = EXCLUDED.permalink,
                permalink_url = EXCLUDED.permalink_url,
                avatar_url = EXCLUDED.avatar_url,
                country = EXCLUDED.country,
                city = EXCLUDED.city,
                description = EXCLUDED.description,
                verified = EXCLUDED.verified,
                followers_count = COALESCE(EXCLUDED.followers_count, users.followers_count),
                followings_count = COALESCE(EXCLUDED.followings_count, users.followings_count),
                tracks_count = COALESCE(EXCLUDED.tracks_count, users.tracks_count),
                playlists_count = COALESCE(EXCLUDED.playlists_count, users.playlists_count),
                reposts_count = COALESCE(EXCLUDED.reposts_count, users.reposts_count),
                comments_count = COALESCE(EXCLUDED.comments_count, users.comments_count),
                kind = EXCLUDED.kind,
                sc_created_at = COALESCE(EXCLUDED.sc_created_at, users.sc_created_at),
                sc_last_modified = COALESCE(EXCLUDED.sc_last_modified, users.sc_last_modified),
                sc_synced_at = now(),
                updated_at = now()
             RETURNING (xmax = 0) AS was_new",
        )
        .bind(&fields.sc_user_id)
        .bind(&fields.urn)
        .bind(&fields.username)
        .bind(&fields.username_normalized)
        .bind(&fields.full_name)
        .bind(&fields.first_name)
        .bind(&fields.last_name)
        .bind(&fields.permalink)
        .bind(&fields.permalink_url)
        .bind(&fields.avatar_url)
        .bind(&fields.country)
        .bind(&fields.city)
        .bind(&fields.description)
        .bind(fields.verified)
        .bind(fields.followers_count)
        .bind(fields.followings_count)
        .bind(fields.tracks_count)
        .bind(fields.playlists_count)
        .bind(fields.reposts_count)
        .bind(fields.comments_count)
        .bind(&fields.kind)
        .bind(fields.sc_created_at)
        .bind(fields.sc_last_modified)
        .fetch_one(&self.pg)
        .await?;
        Ok(row.0)
    }
}

struct ScUserFields {
    sc_user_id: String,
    urn: String,
    username: String,
    username_normalized: String,
    full_name: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    permalink: Option<String>,
    permalink_url: Option<String>,
    avatar_url: Option<String>,
    country: Option<String>,
    city: Option<String>,
    description: Option<String>,
    verified: bool,
    followers_count: Option<i64>,
    followings_count: Option<i64>,
    tracks_count: Option<i64>,
    playlists_count: Option<i64>,
    reposts_count: Option<i64>,
    comments_count: Option<i64>,
    kind: Option<String>,
    sc_created_at: Option<DateTime<Utc>>,
    sc_last_modified: Option<DateTime<Utc>>,
}

impl ScUserFields {
    fn from_sc(payload: &Value) -> Option<Self> {
        let urn = payload.get("urn").and_then(|v| v.as_str())?.to_string();
        if urn.is_empty() {
            return None;
        }
        let sc_user_id = crate::common::sc_ids::extract_sc_id(&urn).to_string();
        let username = payload
            .get("username")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if username.is_empty() {
            return None;
        }
        let username_normalized = crate::modules::enrich::normalize::normalize_name(&username);

        let full_name = string_field(payload, "full_name");
        let first_name = string_field(payload, "first_name");
        let last_name = string_field(payload, "last_name");
        let permalink = string_field(payload, "permalink");
        let permalink_url = string_field(payload, "permalink_url");
        let avatar_url = string_field(payload, "avatar_url");
        let country = string_field(payload, "country_code").or_else(|| string_field(payload, "country"));
        let city = string_field(payload, "city");
        let description = string_field(payload, "description");
        let verified = payload.get("verified").and_then(|v| v.as_bool()).unwrap_or(false);
        let kind = string_field(payload, "kind");
        let sc_created_at = parse_dt(payload.get("created_at"));
        let sc_last_modified = parse_dt(payload.get("last_modified"));

        Some(Self {
            sc_user_id,
            urn,
            username,
            username_normalized,
            full_name,
            first_name,
            last_name,
            permalink,
            permalink_url,
            avatar_url,
            country,
            city,
            description,
            verified,
            followers_count: payload.get("followers_count").and_then(|v| v.as_i64()),
            followings_count: payload.get("followings_count").and_then(|v| v.as_i64()),
            tracks_count: payload.get("track_count").and_then(|v| v.as_i64()),
            playlists_count: payload.get("playlist_count").and_then(|v| v.as_i64()),
            reposts_count: payload.get("reposts_count").and_then(|v| v.as_i64()),
            comments_count: payload.get("comments_count").and_then(|v| v.as_i64()),
            kind,
            sc_created_at,
            sc_last_modified,
        })
    }
}

/// Проекция в SC-shape v1 user payload.
pub fn project_to_sc_shape(row: &UserRow) -> Value {
    let mut obj = Map::new();
    obj.insert("kind".into(), Value::String("user".into()));
    obj.insert("id".into(), parse_id_or_string(&row.sc_user_id));
    obj.insert("urn".into(), Value::String(row.urn.clone()));
    obj.insert("username".into(), Value::String(row.username.clone()));
    if let Some(n) = &row.full_name {
        obj.insert("full_name".into(), Value::String(n.clone()));
    }
    if let Some(n) = &row.first_name {
        obj.insert("first_name".into(), Value::String(n.clone()));
    }
    if let Some(n) = &row.last_name {
        obj.insert("last_name".into(), Value::String(n.clone()));
    }
    if let Some(p) = &row.permalink {
        obj.insert("permalink".into(), Value::String(p.clone()));
    }
    if let Some(p) = &row.permalink_url {
        obj.insert("permalink_url".into(), Value::String(p.clone()));
    }
    if let Some(a) = &row.avatar_url {
        obj.insert("avatar_url".into(), Value::String(a.clone()));
    }
    if let Some(c) = &row.country {
        obj.insert("country_code".into(), Value::String(c.clone()));
    }
    if let Some(c) = &row.city {
        obj.insert("city".into(), Value::String(c.clone()));
    }
    if let Some(d) = &row.description {
        obj.insert("description".into(), Value::String(d.clone()));
    }
    obj.insert("verified".into(), Value::Bool(row.verified));
    obj.insert(
        "followers_count".into(),
        row.followers_count.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    obj.insert(
        "followings_count".into(),
        row.followings_count.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    obj.insert(
        "track_count".into(),
        row.tracks_count.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    obj.insert(
        "playlist_count".into(),
        row.playlists_count.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    obj.insert(
        "reposts_count".into(),
        row.reposts_count.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    obj.insert(
        "comments_count".into(),
        row.comments_count.map(|v| json!(v)).unwrap_or(Value::Null),
    );
    if let Some(t) = row.sc_created_at {
        obj.insert("created_at".into(), Value::String(t.to_rfc3339()));
    }
    if let Some(t) = row.sc_last_modified {
        obj.insert("last_modified".into(), Value::String(t.to_rfc3339()));
    }
    Value::Object(obj)
}
