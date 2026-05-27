//! Сырые SQL-запросы под `/search/db/*`. Каждая выдача — короткая транзакция
//! с локальным `statement_timeout`, чтобы случайный матч на сотни тысяч строк
//! не блокировал пул, а отвалился клиенту 504-м/пустым результатом.
//!
//! Все substring-фильтры рассчитаны на GIN/trgm-индексы из миграции
//! 0022_search_indexes.sql. Поиски от 2 символов; короче — caller отбрасывает
//! запрос до SQL-уровня.
//!
//! Пагинация — offset-based с жёстким max-page (см. handlers). Cursor-style
//! отдельная боль для гибридных ORDER BY (popularity + tiebreaker), а для
//! поискового UX 25 страниц × 20 элементов уже за глаза.

use chrono::{DateTime, NaiveDate, Utc};
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::playlists::{project_to_sc_shape as project_playlist, PlaylistRow};
use crate::modules::tracks::{project_to_sc_shape as project_track, TrackRow};
use crate::modules::users::{project_to_sc_shape as project_user, UserRow};

/// Защитный таймаут на одну выдачу. SET LOCAL — действует только внутри
/// транзакции, не загрязняет сессию пула.
pub const STATEMENT_TIMEOUT_MS: i32 = 2500;

/// Подстрочная заготовка. Лежит как отдельный шаг, чтобы caller-логика не
/// дублировала `format!("%{}%", lower)` на каждом сайте использования.
pub fn like_needle(q: &str) -> String {
    let lower = q.trim().to_lowercase();
    format!("%{lower}%")
}

async fn set_statement_timeout(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> AppResult<()> {
    sqlx::query(&format!(
        "SET LOCAL statement_timeout = {STATEMENT_TIMEOUT_MS}"
    ))
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Поиск треков. `user_urn_filter` ограничивает выдачу uploader'ом (для
/// inline-поиска на UserPage). При пустом фильтре — глобальный скан по
/// trgm-индексу `tracks_search_title_norm_trgm`.
pub async fn search_tracks(
    pg: &PgPool,
    q_lower: &str,
    user_sc_id_filter: Option<&str>,
    page: i64,
    limit: i64,
) -> AppResult<(Vec<Value>, bool)> {
    let needle = like_needle(q_lower);
    let offset = page * limit;

    let mut tx = pg.begin().await?;
    set_statement_timeout(&mut tx).await?;

    // limit+1 → знаем has_more без COUNT(*).
    let fetch_limit = limit + 1;

    let rows: Vec<TrackRow> = if let Some(uid) = user_sc_id_filter {
        // Per-user scope: фильтр на uploader_sc_user_id первый, потом ILIKE.
        // Индекс `tracks_uploader_popular_idx` даёт быстрый старт по uploader'у,
        // фильтр trgm применяется по уже отрезанному набору.
        sqlx::query_as(
            "SELECT * FROM tracks \
             WHERE uploader_sc_user_id = $1 \
               AND sharing = 'public' \
               AND (title_normalized LIKE $2 OR LOWER(title) LIKE $2) \
             ORDER BY play_count_sc DESC NULLS LAST, sc_synced_at DESC, id DESC \
             LIMIT $3 OFFSET $4",
        )
        .bind(uid)
        .bind(&needle)
        .bind(fetch_limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?
    } else {
        // Глобальный поиск. trgm-индекс `tracks_search_title_norm_trgm`
        // подхватывается планировщиком на `title_normalized LIKE`, аплоадер —
        // вспомогательный матч (`tracks_search_uploader_username_trgm`).
        sqlx::query_as(
            "SELECT * FROM tracks \
             WHERE sharing = 'public' \
               AND ( \
                   title_normalized LIKE $1 \
                   OR LOWER(title) LIKE $1 \
                   OR LOWER(uploader_username) LIKE $1 \
               ) \
             ORDER BY play_count_sc DESC NULLS LAST, sc_synced_at DESC, id DESC \
             LIMIT $2 OFFSET $3",
        )
        .bind(&needle)
        .bind(fetch_limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?
    };

    tx.commit().await?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<TrackRow> = rows.into_iter().take(limit as usize).collect();
    let projected = project_tracks_with_uploaders(pg, rows).await?;
    Ok((projected, has_more))
}

/// Один JOIN на uploaders, чтобы каждая карточка трека выходила с
/// полноценным `user` блоком (нужен фронту: avatar, username, country).
async fn project_tracks_with_uploaders(pg: &PgPool, rows: Vec<TrackRow>) -> AppResult<Vec<Value>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let uploader_ids: Vec<String> = rows
        .iter()
        .filter_map(|r| r.uploader_sc_user_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let user_map: std::collections::HashMap<String, Value> = if uploader_ids.is_empty() {
        Default::default()
    } else {
        let users: Vec<UserRow> = sqlx::query_as("SELECT * FROM users WHERE sc_user_id = ANY($1)")
            .bind(&uploader_ids)
            .fetch_all(pg)
            .await?;
        users
            .into_iter()
            .map(|u| (u.sc_user_id.clone(), project_user(&u)))
            .collect()
    };

    Ok(rows
        .into_iter()
        .map(|row| {
            let uploader = row
                .uploader_sc_user_id
                .as_deref()
                .and_then(|uid| user_map.get(uid));
            project_track(&row, uploader)
        })
        .collect())
}

/// Поиск плейлистов. `user_urn_filter` ограничивает выдачу owner'ом.
pub async fn search_playlists(
    pg: &PgPool,
    q_lower: &str,
    user_sc_id_filter: Option<&str>,
    page: i64,
    limit: i64,
) -> AppResult<(Vec<Value>, bool)> {
    let needle = like_needle(q_lower);
    let offset = page * limit;

    let mut tx = pg.begin().await?;
    set_statement_timeout(&mut tx).await?;

    let fetch_limit = limit + 1;

    let rows: Vec<PlaylistRow> = if let Some(uid) = user_sc_id_filter {
        sqlx::query_as(
            "SELECT * FROM playlists \
             WHERE owner_sc_user_id = $1 \
               AND sharing = 'public' \
               AND (title_normalized LIKE $2 OR LOWER(title) LIKE $2) \
             ORDER BY likes_count_sc DESC NULLS LAST, sc_synced_at DESC, urn DESC \
             LIMIT $3 OFFSET $4",
        )
        .bind(uid)
        .bind(&needle)
        .bind(fetch_limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?
    } else {
        sqlx::query_as(
            "SELECT * FROM playlists \
             WHERE sharing = 'public' \
               AND ( \
                   title_normalized LIKE $1 \
                   OR LOWER(title) LIKE $1 \
                   OR LOWER(owner_username) LIKE $1 \
               ) \
             ORDER BY likes_count_sc DESC NULLS LAST, sc_synced_at DESC, urn DESC \
             LIMIT $2 OFFSET $3",
        )
        .bind(&needle)
        .bind(fetch_limit)
        .bind(offset)
        .fetch_all(&mut *tx)
        .await?
    };

    tx.commit().await?;

    let has_more = rows.len() as i64 > limit;
    let rows: Vec<PlaylistRow> = rows.into_iter().take(limit as usize).collect();
    let projected = project_playlists_with_owners(pg, rows).await?;
    Ok((projected, has_more))
}

async fn project_playlists_with_owners(
    pg: &PgPool,
    rows: Vec<PlaylistRow>,
) -> AppResult<Vec<Value>> {
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let owner_ids: Vec<String> = rows
        .iter()
        .filter_map(|r| r.owner_sc_user_id.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();

    let owner_map: std::collections::HashMap<String, Value> = if owner_ids.is_empty() {
        Default::default()
    } else {
        let users: Vec<UserRow> = sqlx::query_as("SELECT * FROM users WHERE sc_user_id = ANY($1)")
            .bind(&owner_ids)
            .fetch_all(pg)
            .await?;
        users
            .into_iter()
            .map(|u| (u.sc_user_id.clone(), project_user(&u)))
            .collect()
    };

    Ok(rows
        .into_iter()
        .map(|row| {
            let owner = row
                .owner_sc_user_id
                .as_deref()
                .and_then(|uid| owner_map.get(uid));
            project_playlist(&row, owner)
        })
        .collect())
}

/// Поиск юзеров. Совместимая SC-shape проекция.
pub async fn search_users(
    pg: &PgPool,
    q_lower: &str,
    page: i64,
    limit: i64,
) -> AppResult<(Vec<Value>, bool)> {
    let needle = like_needle(q_lower);
    let offset = page * limit;

    let mut tx = pg.begin().await?;
    set_statement_timeout(&mut tx).await?;

    let fetch_limit = limit + 1;

    let rows: Vec<UserRow> = sqlx::query_as(
        "SELECT * FROM users \
         WHERE username_normalized LIKE $1 \
            OR LOWER(username) LIKE $1 \
         ORDER BY followers_count DESC NULLS LAST, sc_synced_at DESC, sc_user_id DESC \
         LIMIT $2 OFFSET $3",
    )
    .bind(&needle)
    .bind(fetch_limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    let has_more = rows.len() as i64 > limit;
    let collection: Vec<Value> = rows
        .into_iter()
        .take(limit as usize)
        .map(|r| project_user(&r))
        .collect();
    Ok((collection, has_more))
}

/// Поиск артистов (enrich-сущность). Используются те же поля, что и
/// `/discover/artists` — фронту удобно переиспользовать карточку.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ArtistSearchRow {
    pub id: Uuid,
    pub name: String,
    pub country: Option<String>,
    pub avatar_url: Option<String>,
    pub confidence: f32,
    pub track_count_primary: i32,
    pub track_count_featured: i32,
    pub album_count_denorm: i32,
    pub monthly_listeners: i64,
    pub trending_score: f32,
    pub tags: Vec<String>,
    pub is_star: bool,
    pub star_aura_id: Option<String>,
    pub star_custom_hex: Option<String>,
}

pub async fn search_artists(
    pg: &PgPool,
    q_lower: &str,
    page: i64,
    limit: i64,
) -> AppResult<(Vec<ArtistSearchRow>, bool)> {
    let needle = like_needle(q_lower);
    let offset = page * limit;

    let mut tx = pg.begin().await?;
    set_statement_timeout(&mut tx).await?;

    let fetch_limit = limit + 1;

    let rows: Vec<ArtistSearchRow> = sqlx::query_as(
        "SELECT id, name, country, avatar_url, confidence, \
                track_count_primary, track_count_featured, album_count_denorm, \
                monthly_listeners, trending_score, tags, \
                is_star, star_aura_id, star_custom_hex \
         FROM artists \
         WHERE merged_into IS NULL \
           AND (track_count_primary > 0 OR track_count_featured > 0) \
           AND (normalized_name LIKE $1 OR LOWER(name) LIKE $1) \
         ORDER BY monthly_listeners DESC, trending_score DESC, normalized_name ASC, id ASC \
         LIMIT $2 OFFSET $3",
    )
    .bind(&needle)
    .bind(fetch_limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    let has_more = rows.len() as i64 > limit;
    Ok((rows.into_iter().take(limit as usize).collect(), has_more))
}

/// Поиск альбомов. Возвращает поля совместимые с `/discover/albums` для
/// переиспользования FE-карточки.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AlbumSearchRow {
    pub id: Uuid,
    pub title: String,
    pub kind: String,
    pub release_year: Option<i16>,
    pub release_date: Option<NaiveDate>,
    pub cover_url: Option<String>,
    pub confidence: f32,
    pub track_count: i32,
    pub total_duration_ms: i64,
    pub popularity_score: f32,
    pub is_star_artist: bool,
    pub primary_artist_id: Option<Uuid>,
    pub primary_artist_name: Option<String>,
    pub primary_artist_avatar: Option<String>,
}

pub async fn search_albums(
    pg: &PgPool,
    q_lower: &str,
    page: i64,
    limit: i64,
) -> AppResult<(Vec<AlbumSearchRow>, bool)> {
    let needle = like_needle(q_lower);
    let offset = page * limit;

    let mut tx = pg.begin().await?;
    set_statement_timeout(&mut tx).await?;

    let fetch_limit = limit + 1;

    let rows: Vec<AlbumSearchRow> = sqlx::query_as(
        "SELECT al.id, al.title, al.type AS kind, al.release_year, al.release_date, \
                al.cover_url, al.confidence, al.track_count, al.total_duration_ms, \
                al.popularity_score, al.is_star_artist, al.primary_artist_id, \
                a.name AS primary_artist_name, a.avatar_url AS primary_artist_avatar \
         FROM albums al \
         LEFT JOIN artists a ON a.id = al.primary_artist_id AND a.merged_into IS NULL \
         WHERE al.track_count > 0 \
           AND ( \
               al.normalized_title LIKE $1 \
               OR LOWER(al.title) LIKE $1 \
               OR LOWER(COALESCE(a.name, '')) LIKE $1 \
           ) \
         ORDER BY al.popularity_score DESC, al.normalized_title ASC, al.id ASC \
         LIMIT $2 OFFSET $3",
    )
    .bind(&needle)
    .bind(fetch_limit)
    .bind(offset)
    .fetch_all(&mut *tx)
    .await?;

    tx.commit().await?;

    let has_more = rows.len() as i64 > limit;
    Ok((rows.into_iter().take(limit as usize).collect(), has_more))
}

/// Резолв `user_urn` → `sc_user_id`. Возвращает None если такого юзера у нас
/// в зеркале нет (тогда фильтр по user_urn равнозначен пустой выдаче).
pub async fn resolve_user_sc_id(pg: &PgPool, user_urn: &str) -> AppResult<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT sc_user_id FROM users WHERE urn = $1")
        .bind(user_urn)
        .fetch_optional(pg)
        .await?;
    Ok(row.map(|(id,)| id))
}

/// Хелпер для frontend: вернуть `synced_at` репозиториев — фронт может
/// показывать "база обновлена ⨯ часов назад" если очень захочет. На MVP не
/// используем, но полезный seam.
#[allow(dead_code)]
pub async fn db_last_synced(pg: &PgPool) -> AppResult<Option<DateTime<Utc>>> {
    let row: Option<(Option<DateTime<Utc>>,)> =
        sqlx::query_as("SELECT MAX(sc_synced_at) FROM tracks")
            .fetch_optional(pg)
            .await?;
    Ok(row.and_then(|(t,)| t))
}
