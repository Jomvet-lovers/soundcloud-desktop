use std::sync::Arc;

use chrono::NaiveDateTime;
use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use sqlx::FromRow;
use tracing::warn;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::modules::auth::TokenKind;
use crate::modules::likes::cold as likes_cold;
use crate::sc::ScReadService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeaturedItemType {
    Track,
    Playlist,
    User,
}

impl FeaturedItemType {
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
    read: Arc<ScReadService>,
}

impl FeaturedService {
    pub fn new(pg: sqlx::PgPool, read: Arc<ScReadService>) -> Arc<Self> {
        Arc::new(Self { pg, read })
    }

    pub async fn find_all(&self) -> AppResult<Vec<FeaturedItem>> {
        let rows = sqlx::query_file!("queries/featured/service/find_all.sql")
            .fetch_all(&self.pg)
            .await?;
        Ok(rows
            .into_iter()
            .map(|r| FeaturedItem {
                id: r.id,
                type_: r.item_type,
                sc_urn: r.sc_urn,
                weight: r.weight,
                active: r.active,
                created_at: r.created_at,
            })
            .collect())
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
        let row = sqlx::query_file!(
            "queries/featured/service/create.sql",
            type_,
            sc_urn,
            weight.unwrap_or(1),
            active.unwrap_or(true)
        )
        .fetch_one(&self.pg)
        .await?;
        Ok(FeaturedItem {
            id: row.id,
            type_: row.item_type,
            sc_urn: row.sc_urn,
            weight: row.weight,
            active: row.active,
            created_at: row.created_at,
        })
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
        sqlx::query_file!("queries/featured/service/remove.sql", uuid)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    pub async fn pick(
        &self,
        session_id: &str,
        sc_user_id: &str,
    ) -> AppResult<Option<FeaturedResult>> {
        let items: Vec<FeaturedItem> =
            sqlx::query_file!("queries/featured/service/pick_active.sql")
                .fetch_all(&self.pg)
                .await?
                .into_iter()
                .map(|r| FeaturedItem {
                    id: r.id,
                    type_: r.item_type,
                    sc_urn: r.sc_urn,
                    weight: r.weight,
                    active: r.active,
                    created_at: r.created_at,
                })
                .collect();
        if items.is_empty() {
            return Ok(None);
        }

        let picked = weighted_random(&items);
        let session_uuid = Uuid::parse_str(session_id)
            .map_err(|_| AppError::unauthorized("Malformed session id"))?;
        let kind = TokenKind::UserFirst(session_uuid);

        match self.resolve(picked, kind, sc_user_id).await {
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
        kind: TokenKind,
        sc_user_id: &str,
    ) -> AppResult<FeaturedResult> {
        let id = crate::common::sc_ids::extract_sc_id(&item.sc_urn);
        match item.type_.as_str() {
            "track" => {
                let track = self.read.track_by_id(kind, id).await?;
                let mut single = vec![track];
                likes_cold::apply_user_favorite_flag(&self.pg, sc_user_id, &mut single).await?;
                Ok(FeaturedResult {
                    type_: "track".into(),
                    data: single.into_iter().next().unwrap_or(Value::Null),
                })
            }
            "playlist" => {
                let mut playlist = self.read.playlist_meta(kind, id).await?;
                // Featured cards expect full tracks (apiv1 embedded them); hydrate via
                // apiv2 best-effort so the card isn't left with id-stubs.
                if let Ok(tracks) = self.read.playlist_tracks(id).await {
                    if let Some(obj) = playlist.as_object_mut() {
                        obj.insert("tracks".into(), Value::Array(tracks));
                    }
                }
                Ok(FeaturedResult {
                    type_: "playlist".into(),
                    data: playlist,
                })
            }
            "user" => {
                let user = self.read.user_by_id(kind, id).await?;
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
