use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::common::admin::AdminAuth;
use crate::error::{AppError, AppResult};
use crate::modules::auth::TokenKind;
use crate::modules::enrich::artist_names::{self, RawMetaMatch};
use crate::modules::enrich::normalize::normalize_name;
use crate::state::AppState;

// ───────────────────────── resolve by URL ─────────────────────────

#[derive(Deserialize)]
pub struct ResolveQuery {
    pub url: String,
}

#[derive(Serialize)]
pub struct ResolveResult {
    pub kind: String,
    pub id: String,
    pub urn: String,
    pub title: Option<String>,
    pub username: Option<String>,
    pub permalink_url: Option<String>,
    pub artwork_url: Option<String>,
}

fn value_id(v: &Value) -> String {
    match v.get("id") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// GET /admin/resolve?url= — resolve any SoundCloud URL to its kind + canonical
/// URN so the UI can auto-fill track/playlist/user pickers from a pasted link.
#[tracing::instrument(skip_all)]
pub async fn resolve(
    _: AdminAuth,
    State(st): State<AppState>,
    Query(q): Query<ResolveQuery>,
) -> AppResult<Json<ResolveResult>> {
    let url = q.url.trim();
    if url.is_empty() {
        return Err(AppError::bad_request("url is required"));
    }
    let v: Value = st.resolve.resolve(TokenKind::PublicPool, url).await?;
    let kind = v
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let id = value_id(&v);
    let collection = match kind.as_str() {
        "track" => "tracks",
        "playlist" | "system-playlist" => "playlists",
        "user" => "users",
        _ => "",
    };
    let urn = if !collection.is_empty() && !id.is_empty() {
        format!("soundcloud:{collection}:{id}")
    } else {
        String::new()
    };
    Ok(Json(ResolveResult {
        kind,
        id,
        urn,
        title: v.get("title").and_then(Value::as_str).map(str::to_string),
        username: v
            .get("username")
            .and_then(Value::as_str)
            .map(str::to_string),
        permalink_url: v
            .get("permalink_url")
            .and_then(Value::as_str)
            .map(str::to_string),
        artwork_url: v
            .get("artwork_url")
            .and_then(Value::as_str)
            .or_else(|| v.get("avatar_url").and_then(Value::as_str))
            .map(str::to_string),
    }))
}

// ───────────────────────── artists ─────────────────────────

#[derive(Deserialize)]
pub struct ArtistsQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ArtistListRow {
    pub id: Uuid,
    pub name: String,
    pub country: Option<String>,
    pub avatar_url: Option<String>,
    pub confidence: f32,
    pub sc_user_id: Option<String>,
    pub source: String,
    pub track_count: i64,
    pub sc_accounts_count: i64,
}

#[tracing::instrument(skip_all)]
pub async fn artists_search(
    _: AdminAuth,
    State(st): State<AppState>,
    Query(q): Query<ArtistsQuery>,
) -> AppResult<Json<Vec<ArtistListRow>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let term = q.q.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let like = term.as_ref().map(|s| format!("%{s}%"));

    let rows = sqlx::query_file_as!(
        ArtistListRow,
        "queries/admin/catalog/artists_search.sql",
        like,
        term,
        limit
    )
    .fetch_all(&st.pg)
    .await?;
    Ok(Json(rows))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct ArtistRow {
    pub id: Uuid,
    pub name: String,
    pub normalized_name: String,
    pub country: Option<String>,
    pub avatar_url: Option<String>,
    pub bio: Option<String>,
    pub sc_user_id: Option<String>,
    pub source: String,
    pub confidence: f32,
    pub mb_artist_id: Option<String>,
    pub spotify_artist_id: Option<String>,
    pub genius_artist_id: Option<String>,
    pub merged_into: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

const ARTIST_COLS: &str = "id, name, normalized_name, country, avatar_url, bio, sc_user_id, source, \
     confidence, mb_artist_id, spotify_artist_id, genius_artist_id, merged_into, created_at, updated_at";

#[derive(Serialize, sqlx::FromRow)]
pub struct ScAccountRow {
    pub sc_user_id: String,
    pub role: String,
    pub source: String,
    pub verified: bool,
    pub notes: Option<String>,
}

#[derive(Serialize)]
pub struct ArtistDetail {
    #[serde(flatten)]
    pub artist: ArtistRow,
    pub sc_accounts: Vec<ScAccountRow>,
    pub track_count: i64,
    pub album_count: i64,
}

#[tracing::instrument(skip_all)]
pub async fn artist_detail(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(artist_id): Path<Uuid>,
) -> AppResult<Json<ArtistDetail>> {
    let artist = sqlx::query_file_as!(ArtistRow, "queries/admin/catalog/artist_get.sql", artist_id)
        .fetch_optional(&st.pg)
        .await?
        .ok_or_else(|| AppError::not_found("artist not found"))?;

    let sc_accounts = sqlx::query_file_as!(
        ScAccountRow,
        "queries/admin/catalog/artist_sc_accounts.sql",
        artist_id
    )
    .fetch_all(&st.pg)
    .await?;

    let track_count: i64 =
        sqlx::query_file_scalar!("queries/admin/catalog/artist_track_count.sql", artist_id)
            .fetch_one(&st.pg)
            .await?;
    let album_count: i64 =
        sqlx::query_file_scalar!("queries/admin/catalog/artist_album_count.sql", artist_id)
            .fetch_one(&st.pg)
            .await?;

    Ok(Json(ArtistDetail {
        artist,
        sc_accounts,
        track_count,
        album_count,
    }))
}

#[derive(Deserialize)]
pub struct CreateArtist {
    pub name: String,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub sc_user_id: Option<String>,
}

#[tracing::instrument(skip_all)]
pub async fn artist_create(
    _: AdminAuth,
    State(st): State<AppState>,
    Json(body): Json<CreateArtist>,
) -> AppResult<Json<ArtistRow>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::bad_request("name is required"));
    }
    let normalized = normalize_name(name);
    if normalized.is_empty() {
        return Err(AppError::bad_request("name normalizes to empty"));
    }

    let exists: bool =
        sqlx::query_file_scalar!("queries/admin/catalog/artist_name_exists.sql", &normalized)
            .fetch_one(&st.pg)
            .await?;
    if exists {
        return Err(AppError::bad_request(
            "artist with this name already exists",
        ));
    }

    let row = sqlx::query_as::<_, ArtistRow>(&format!(
        "INSERT INTO artists (name, normalized_name, country, bio, avatar_url, sc_user_id, source, confidence) \
         VALUES ($1, $2, $3, $4, $5, $6, 'manual', 1.0) RETURNING {ARTIST_COLS}"
    ))
        .bind(name)
        .bind(&normalized)
        .bind(&body.country)
        .bind(&body.bio)
        .bind(&body.avatar_url)
        .bind(&body.sc_user_id)
        .fetch_one(&st.pg)
        .await?;
    Ok(Json(row))
}

#[derive(Deserialize)]
pub struct UpdateArtist {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub country: Option<String>,
    #[serde(default)]
    pub bio: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
    #[serde(default)]
    pub sc_user_id: Option<String>,
    #[serde(default)]
    pub confidence: Option<f32>,
}

#[tracing::instrument(skip_all)]
pub async fn artist_update(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(artist_id): Path<Uuid>,
    Json(body): Json<UpdateArtist>,
) -> AppResult<Json<ArtistRow>> {
    let name = body
        .name
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let normalized = name.as_deref().map(normalize_name);

    let row = sqlx::query_file_as!(
        ArtistRow,
        "queries/admin/catalog/artist_update.sql",
        artist_id,
        name.as_deref(),
        normalized.as_deref(),
        body.country.as_deref(),
        body.bio.as_deref(),
        body.avatar_url.as_deref(),
        body.sc_user_id.as_deref(),
        body.confidence
    )
    .fetch_optional(&st.pg)
    .await?
    .ok_or_else(|| AppError::not_found("artist not found"))?;
    Ok(Json(row))
}

// ───────────────────────── albums ─────────────────────────

#[derive(Deserialize)]
pub struct AlbumsQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct AlbumListRow {
    pub id: Uuid,
    pub title: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub release_year: Option<i16>,
    pub primary_artist_id: Option<Uuid>,
    pub primary_artist_name: Option<String>,
    pub track_count: i64,
}

#[tracing::instrument(skip_all)]
pub async fn albums_search(
    _: AdminAuth,
    State(st): State<AppState>,
    Query(q): Query<AlbumsQuery>,
) -> AppResult<Json<Vec<AlbumListRow>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let like =
        q.q.map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(|s| format!("%{s}%"));

    let rows = sqlx::query_file_as!(
        AlbumListRow,
        "queries/admin/catalog/albums_search.sql",
        like,
        limit
    )
    .fetch_all(&st.pg)
    .await?;
    Ok(Json(rows))
}

// ───────────────────────── tracks ─────────────────────────

#[derive(Deserialize)]
pub struct TracksQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct TrackListRow {
    pub id: Uuid,
    pub sc_track_id: String,
    pub title: String,
    pub metadata_artist: Option<String>,
    pub artwork_url: Option<String>,
    pub primary_artist_id: Option<Uuid>,
    pub primary_artist_name: Option<String>,
    pub album_id: Option<Uuid>,
    pub album_title: Option<String>,
    pub enrich_state: String,
    pub release_year: Option<i16>,
}

/// Строка триажа: трек + вердикт сравнения распознанных артистов с RAW-метой.
/// Вердикт считается здесь, на бэке, тем же `artist_names`-алгоритмом, что и
/// resolver — у админки нет своей логики сравнения.
#[derive(Serialize)]
pub struct TrackListItem {
    #[serde(flatten)]
    pub row: TrackListRow,
    /// match / partial / mismatch; None — меты нет или она мусор.
    pub raw_match: Option<RawMetaMatch>,
    /// Распознанные primary-кредиты (включая co-артистов).
    pub detected_names: Vec<String>,
    /// RAW-мета, распарсенная на имена.
    pub raw_names: Vec<String>,
}

/// Имена primary-кредитов по трекам (для вердикта нужен полный состав,
/// а не только денормализованный `primary_artist_name`).
async fn primary_names_for(
    pg: &sqlx::PgPool,
    track_ids: &[Uuid],
) -> AppResult<std::collections::HashMap<Uuid, Vec<String>>> {
    let mut map: std::collections::HashMap<Uuid, Vec<String>> = std::collections::HashMap::new();
    if track_ids.is_empty() {
        return Ok(map);
    }
    let rows = sqlx::query_file!("queries/admin/catalog/track_primary_names.sql", track_ids)
        .fetch_all(pg)
        .await?;
    for r in rows {
        map.entry(r.track_id).or_default().push(r.name);
    }
    Ok(map)
}

fn to_list_item(row: TrackListRow, credit_names: Option<Vec<String>>) -> TrackListItem {
    let mut detected = credit_names.unwrap_or_default();
    if detected.is_empty() {
        if let Some(n) = row.primary_artist_name.clone() {
            detected.push(n);
        }
    }
    let raw_match = row.metadata_artist.as_deref().and_then(|meta| {
        artist_names::compare_with_meta(detected.iter().map(|s| s.as_str()), meta)
    });
    let raw_names = row
        .metadata_artist
        .as_deref()
        .map(artist_names::meta_artist_names)
        .unwrap_or_default();
    TrackListItem {
        row,
        raw_match,
        detected_names: detected,
        raw_names,
    }
}

#[tracing::instrument(skip_all)]
pub async fn tracks_search(
    _: AdminAuth,
    State(st): State<AppState>,
    Query(q): Query<TracksQuery>,
) -> AppResult<Json<Vec<TrackListItem>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let term = q.q.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let like = term.as_ref().map(|s| format!("%{s}%"));

    let rows = sqlx::query_file_as!(
        TrackListRow,
        "queries/admin/catalog/tracks_search.sql",
        like,
        term,
        limit
    )
    .fetch_all(&st.pg)
    .await?;

    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let mut credits = primary_names_for(&st.pg, &ids).await?;
    let items = rows
        .into_iter()
        .map(|r| {
            let names = credits.remove(&r.id);
            to_list_item(r, names)
        })
        .collect();
    Ok(Json(items))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct TrackCreditRow {
    pub artist_id: Uuid,
    pub name: Option<String>,
    pub role: String,
    pub position: i16,
    pub source: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct BlockRow {
    pub artist_id: Uuid,
    pub name: Option<String>,
    pub note: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Serialize)]
pub struct TrackDetail {
    #[serde(flatten)]
    pub track: TrackListItem,
    pub credits: Vec<TrackCreditRow>,
    pub blocks: Vec<BlockRow>,
}

#[tracing::instrument(skip_all)]
pub async fn track_detail(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(track_id): Path<Uuid>,
) -> AppResult<Json<TrackDetail>> {
    let track = sqlx::query_file_as!(
        TrackListRow,
        "queries/admin/catalog/track_get.sql",
        track_id
    )
    .fetch_optional(&st.pg)
    .await?
    .ok_or_else(|| AppError::not_found("track not found"))?;

    let credits = sqlx::query_file_as!(
        TrackCreditRow,
        "queries/admin/catalog/track_credits.sql",
        track_id
    )
    .fetch_all(&st.pg)
    .await?;

    let blocks = sqlx::query_file_as!(BlockRow, "queries/admin/catalog/track_blocks.sql", track_id)
        .fetch_all(&st.pg)
        .await?;

    let primary_names: Vec<String> = credits
        .iter()
        .filter(|c| c.role == "primary")
        .filter_map(|c| c.name.clone())
        .collect();
    let track = to_list_item(track, Some(primary_names));

    Ok(Json(TrackDetail {
        track,
        credits,
        blocks,
    }))
}

#[derive(Deserialize)]
pub struct SetPrimaryArtist {
    pub artist_id: Uuid,
}

/// PATCH /admin/tracks/{id}/primary-artist — fix a mis-detected primary artist.
/// Updates both the denormalized `tracks.primary_artist_id` and the
/// `track_artists` primary credit, in one transaction.
#[tracing::instrument(skip_all)]
pub async fn track_set_primary_artist(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(track_id): Path<Uuid>,
    Json(body): Json<SetPrimaryArtist>,
) -> AppResult<Json<Value>> {
    let artist_ok: bool =
        sqlx::query_file_scalar!("queries/admin/catalog/artist_exists.sql", body.artist_id)
            .fetch_one(&st.pg)
            .await?;
    if !artist_ok {
        return Err(AppError::bad_request("artist not found"));
    }

    let mut tx = st.pg.begin().await?;
    // An explicit manual assignment lifts any detach-block for this pair.
    sqlx::query_file!(
        "queries/admin/catalog/block_delete_pair.sql",
        track_id,
        body.artist_id
    )
    .execute(&mut *tx)
    .await?;
    let updated = sqlx::query_file!(
        "queries/admin/catalog/track_set_primary_artist.sql",
        body.artist_id,
        track_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::not_found("track not found"));
    }
    sqlx::query_file!(
        "queries/admin/catalog/track_artists_delete_primary.sql",
        track_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "queries/admin/catalog/track_artists_insert_primary.sql",
        track_id,
        body.artist_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct SetAlbum {
    /// null detaches the track from any album.
    #[serde(default)]
    pub album_id: Option<Uuid>,
}

/// PATCH /admin/tracks/{id}/album — fix/clear a mis-detected album. Syncs both
/// `tracks.album_id` and the `album_tracks` join.
#[tracing::instrument(skip_all)]
pub async fn track_set_album(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(track_id): Path<Uuid>,
    Json(body): Json<SetAlbum>,
) -> AppResult<Json<Value>> {
    if let Some(album_id) = body.album_id {
        let album_ok: bool =
            sqlx::query_file_scalar!("queries/admin/catalog/album_exists.sql", album_id)
                .fetch_one(&st.pg)
                .await?;
        if !album_ok {
            return Err(AppError::bad_request("album not found"));
        }
    }

    let mut tx = st.pg.begin().await?;
    let updated = sqlx::query_file!(
        "queries/admin/catalog/track_set_album.sql",
        body.album_id,
        track_id
    )
    .execute(&mut *tx)
    .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::not_found("track not found"));
    }
    sqlx::query_file!(
        "queries/admin/catalog/album_tracks_delete_by_track.sql",
        track_id
    )
    .execute(&mut *tx)
    .await?;
    if let Some(album_id) = body.album_id {
        sqlx::query_file!(
            "queries/admin/catalog/album_tracks_insert.sql",
            album_id,
            track_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "album_id": body.album_id }),
    ))
}

// ───────────────────────── track credits (feat / co-artists) ─────────────────────────

const CREDIT_ROLES: [&str; 4] = ["primary", "feature", "remixer", "producer"];

fn default_feature_role() -> String {
    "feature".to_string()
}

#[derive(Deserialize)]
pub struct AddCredit {
    pub artist_id: Uuid,
    #[serde(default = "default_feature_role")]
    pub role: String,
    #[serde(default)]
    pub position: Option<i16>,
}

/// POST /admin/tracks/{id}/credits — add/upsert a track credit (default role
/// "feature" — featured artists). When role is "primary" it also syncs the
/// denormalized `tracks.primary_artist_id` and drops any other primary credit.
#[tracing::instrument(skip_all)]
pub async fn track_add_credit(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(track_id): Path<Uuid>,
    Json(body): Json<AddCredit>,
) -> AppResult<Json<Value>> {
    let role = body.role.trim().to_lowercase();
    if !CREDIT_ROLES.contains(&role.as_str()) {
        return Err(AppError::bad_request(
            "role must be one of: primary, feature, remixer, producer",
        ));
    }
    let artist_ok: bool =
        sqlx::query_file_scalar!("queries/admin/catalog/artist_exists.sql", body.artist_id)
            .fetch_one(&st.pg)
            .await?;
    if !artist_ok {
        return Err(AppError::bad_request("artist not found"));
    }

    let mut tx = st.pg.begin().await?;
    let track_ok: bool =
        sqlx::query_file_scalar!("queries/admin/catalog/track_exists.sql", track_id)
            .fetch_one(&mut *tx)
            .await?;
    if !track_ok {
        return Err(AppError::not_found("track not found"));
    }

    // An explicit manual credit lifts any detach-block for this pair.
    sqlx::query_file!(
        "queries/admin/catalog/block_delete_pair.sql",
        track_id,
        body.artist_id
    )
    .execute(&mut *tx)
    .await?;

    sqlx::query_file!(
        "queries/admin/catalog/track_artists_upsert_credit.sql",
        track_id,
        body.artist_id,
        &role,
        body.position.map(i32::from)
    )
    .execute(&mut *tx)
    .await?;

    if role == "primary" {
        sqlx::query_file!(
            "queries/admin/catalog/track_artists_delete_other_primary.sql",
            track_id,
            body.artist_id
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query_file!(
            "queries/admin/catalog/track_set_primary_artist_id.sql",
            body.artist_id,
            track_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true, "role": role })))
}

#[derive(Deserialize)]
pub struct CreditQuery {
    #[serde(default = "default_feature_role")]
    pub role: String,
}

/// DELETE /admin/tracks/{id}/credits/{artist_id}?role=feature — remove a credit.
/// Removing the primary also clears `tracks.primary_artist_id` if it matched.
#[tracing::instrument(skip_all)]
pub async fn track_remove_credit(
    _: AdminAuth,
    State(st): State<AppState>,
    Path((track_id, artist_id)): Path<(Uuid, Uuid)>,
    Query(q): Query<CreditQuery>,
) -> AppResult<Json<Value>> {
    let role = q.role.trim().to_lowercase();

    let mut tx = st.pg.begin().await?;
    let res = sqlx::query_file!(
        "queries/admin/catalog/track_artists_delete_credit.sql",
        track_id,
        artist_id,
        &role
    )
    .execute(&mut *tx)
    .await?;
    if role == "primary" {
        sqlx::query_file!(
            "queries/admin/catalog/track_clear_primary_if_match.sql",
            track_id,
            artist_id
        )
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "removed": res.rows_affected() }),
    ))
}

// ───────────────────────── detach (sticky unlink) ─────────────────────────

#[derive(Deserialize)]
pub struct DetachArtist {
    pub artist_id: Uuid,
    #[serde(default)]
    pub note: Option<String>,
}

/// POST /admin/tracks/{id}/detach-artist — permanently unlink an artist from a
/// track: drop all its credits, clear the denormalized primary if it matched,
/// and record a block so the enrich/crawl pipeline never re-links it (triggers).
#[tracing::instrument(skip_all)]
pub async fn track_detach_artist(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(track_id): Path<Uuid>,
    Json(body): Json<DetachArtist>,
) -> AppResult<Json<Value>> {
    let mut tx = st.pg.begin().await?;
    let track_ok: bool =
        sqlx::query_file_scalar!("queries/admin/catalog/track_exists.sql", track_id)
            .fetch_one(&mut *tx)
            .await?;
    if !track_ok {
        return Err(AppError::not_found("track not found"));
    }
    // Runtime query: nullable `note` ($3) — sqlx query! infers INSERT params as
    // non-null (&str), conflicting with Option<String>. Kept on runtime.
    sqlx::query(
        "INSERT INTO track_artist_blocks (track_id, artist_id, note) VALUES ($1, $2, $3) \
         ON CONFLICT (track_id, artist_id) DO UPDATE SET note = EXCLUDED.note",
    )
    .bind(track_id)
    .bind(body.artist_id)
    .bind(&body.note)
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "queries/admin/catalog/track_artists_delete_pair.sql",
        track_id,
        body.artist_id
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query_file!(
        "queries/admin/catalog/track_clear_primary_if_match.sql",
        track_id,
        body.artist_id
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

/// DELETE /admin/tracks/{id}/blocks/{artist_id} — lift a detach block (re-allow linking).
#[tracing::instrument(skip_all)]
pub async fn track_unblock_artist(
    _: AdminAuth,
    State(st): State<AppState>,
    Path((track_id, artist_id)): Path<(Uuid, Uuid)>,
) -> AppResult<Json<Value>> {
    let res = sqlx::query_file!(
        "queries/admin/catalog/block_delete_pair.sql",
        track_id,
        artist_id
    )
    .execute(&st.pg)
    .await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "removed": res.rows_affected() }),
    ))
}
