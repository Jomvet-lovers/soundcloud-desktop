use std::sync::Arc;

use chrono::NaiveDateTime;
use rand::Rng;
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::FromRow;
use tracing::warn;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::modules::auth::AuthService;
use crate::modules::local_likes::LocalLikesService;
use crate::sc::ScClient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeaturedItemType {
    Track,
    Playlist,
    User,
}

impl FeaturedItemType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Track => "track",
            Self::Playlist => "playlist",
            Self::User => "user",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "track" => Some(Self::Track),
            "playlist" => Some(Self::Playlist),
            "user" => Some(Self::User),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize)]
pub struct FeaturedItem {
    pub id: Uuid,
    #[serde(rename = "type")]
    #[sqlx(rename = "type")]
    pub type_: String,
    #[serde(rename = "scUrn")]
    pub sc_urn: String,
    pub weight: i32,
    pub active: bool,
    #[serde(rename = "createdAt")]
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Clone, Serialize)]
pub struct FeaturedResult {
    #[serde(rename = "type")]
    pub type_: String,
    pub data: Value,
}

pub struct FeaturedService {
    pg: sqlx::PgPool,
    sc: ScClient,
    auth: Arc<AuthService>,
    local_likes: Arc<LocalLikesService>,
}

impl FeaturedService {
    pub fn new(
        pg: sqlx::PgPool,
        sc: ScClient,
        auth: Arc<AuthService>,
        local_likes: Arc<LocalLikesService>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            sc,
            auth,
            local_likes,
        })
    }

    pub async fn find_all(&self) -> AppResult<Vec<FeaturedItem>> {
        let rows: Vec<FeaturedItem> = sqlx::query_as(
            r#"SELECT id, "type", sc_urn, weight, active, created_at FROM featured_items ORDER BY created_at DESC"#,
        )
        .fetch_all(&self.pg)
        .await?;
        Ok(rows)
    }

    pub async fn create(
        &self,
        type_: &str,
        sc_urn: &str,
        weight: Option<i32>,
        active: Option<bool>,
    ) -> AppResult<FeaturedItem> {
        if FeaturedItemType::parse(type_).is_none() {
            return Err(AppError::bad_request(
                "type must be one of: track, playlist, user",
            ));
        }
        let row: FeaturedItem = sqlx::query_as(
            r#"INSERT INTO featured_items ("type", sc_urn, weight, active) VALUES ($1, $2, $3, $4) RETURNING id, "type", sc_urn, weight, active, created_at"#,
        )
        .bind(type_)
        .bind(sc_urn)
        .bind(weight.unwrap_or(1))
        .bind(active.unwrap_or(true))
        .fetch_one(&self.pg)
        .await?;
        Ok(row)
    }

    pub async fn update(
        &self,
        id: &str,
        type_: Option<&str>,
        sc_urn: Option<&str>,
        weight: Option<i32>,
        active: Option<bool>,
    ) -> AppResult<FeaturedItem> {
        if let Some(t) = type_ {
            if FeaturedItemType::parse(t).is_none() {
                return Err(AppError::bad_request(
                    "type must be one of: track, playlist, user",
                ));
            }
        }
        let uuid = Uuid::parse_str(id)
            .map_err(|_| AppError::not_found(format!("featured item {id} not found")))?;
        let row: Option<FeaturedItem> = sqlx::query_as(
            r#"UPDATE featured_items SET
                "type" = COALESCE($2, "type"),
                sc_urn = COALESCE($3, sc_urn),
                weight = COALESCE($4, weight),
                active = COALESCE($5, active)
             WHERE id = $1
             RETURNING id, "type", sc_urn, weight, active, created_at"#,
        )
        .bind(uuid)
        .bind(type_)
        .bind(sc_urn)
        .bind(weight)
        .bind(active)
        .fetch_optional(&self.pg)
        .await?;
        row.ok_or_else(|| AppError::not_found(format!("featured item {id} not found")))
    }

    pub async fn remove(&self, id: &str) -> AppResult<()> {
        let uuid = match Uuid::parse_str(id) {
            Ok(u) => u,
            Err(_) => return Ok(()),
        };
        sqlx::query("DELETE FROM featured_items WHERE id = $1")
            .bind(uuid)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    pub async fn pick(
        &self,
        session_id: &str,
        sc_user_id: &str,
    ) -> AppResult<Option<FeaturedResult>> {
        let items: Vec<FeaturedItem> = sqlx::query_as(
            r#"SELECT id, "type", sc_urn, weight, active, created_at FROM featured_items WHERE active = true"#,
        )
        .fetch_all(&self.pg)
        .await?;
        if items.is_empty() {
            return Ok(None);
        }

        let picked = weighted_random(&items);
        let session_uuid = Uuid::parse_str(session_id)
            .map_err(|_| AppError::unauthorized("Malformed session id"))?;
        let token = self.auth.get_valid_access_token(session_uuid).await?;

        match self.resolve(picked, &token, sc_user_id).await {
            Ok(r) => Ok(Some(r)),
            Err(e) => {
                warn!(
                    type_ = %picked.type_,
                    sc_urn = %picked.sc_urn,
                    error = %e,
                    "Failed to resolve featured"
                );
                Ok(None)
            }
        }
    }

    async fn resolve(
        &self,
        item: &FeaturedItem,
        token: &str,
        sc_user_id: &str,
    ) -> AppResult<FeaturedResult> {
        match item.type_.as_str() {
            "track" => {
                let mut track: Value = self
                    .sc
                    .api_get_value(&format!("/tracks/{}", item.sc_urn), token, None)
                    .await?;
                if let Some(urn) = track.get("urn").and_then(|v| v.as_str()).map(String::from) {
                    let liked = self
                        .local_likes
                        .get_liked_track_ids(sc_user_id, &[urn.clone()])
                        .await?;
                    if liked.contains(&urn) {
                        if let Some(obj) = track.as_object_mut() {
                            obj.insert("user_favorite".into(), json!(true));
                        }
                    }
                }
                Ok(FeaturedResult {
                    type_: "track".into(),
                    data: track,
                })
            }
            "playlist" => {
                let playlist: Value = self
                    .sc
                    .api_get_value(&format!("/playlists/{}", item.sc_urn), token, None)
                    .await?;
                Ok(FeaturedResult {
                    type_: "playlist".into(),
                    data: playlist,
                })
            }
            "user" => {
                let user: Value = self
                    .sc
                    .api_get_value(&format!("/users/{}", item.sc_urn), token, None)
                    .await?;
                Ok(FeaturedResult {
                    type_: "user".into(),
                    data: user,
                })
            }
            other => Err(AppError::internal(format!(
                "unknown featured type: {other}"
            ))),
        }
    }
}

fn weighted_random(items: &[FeaturedItem]) -> &FeaturedItem {
    let total: i64 = items.iter().map(|i| i.weight.max(1) as i64).sum();
    if total <= 0 {
        return items.last().expect("featured list non-empty");
    }
    let mut rng = rand::thread_rng();
    let mut rand: i64 = rng.gen_range(0..total);
    for item in items {
        rand -= item.weight.max(1) as i64;
        if rand < 0 {
            return item;
        }
    }
    items.last().expect("featured list non-empty")
}
