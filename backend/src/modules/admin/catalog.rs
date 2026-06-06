use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::common::admin::AdminAuth;
use crate::error::{AppError, AppResult};
use crate::modules::auth::TokenKind;
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
    let kind = v.get("kind").and_then(Value::as_str).unwrap_or("").to_string();
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
        username: v.get("username").and_then(Value::as_str).map(str::to_string),
        permalink_url: v.get("permalink_url").and_then(Value::as_str).map(str::to_string),
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

    let rows = sqlx::query_as::<_, ArtistListRow>(
        "SELECT a.id, a.name, a.country, a.avatar_url, a.confidence, a.sc_user_id, a.source, \
                (SELECT COUNT(*)::int8 FROM track_artists ta WHERE ta.artist_id = a.id) AS track_count, \
                (SELECT COUNT(*)::int8 FROM artist_sc_accounts s WHERE s.artist_id = a.id) AS sc_accounts_count \
         FROM artists a \
         WHERE a.merged_into IS NULL \
           AND ($1::text IS NULL OR a.name ILIKE $1 OR a.sc_user_id = $2) \
         ORDER BY a.confidence DESC, a.name ASC \
         LIMIT $3",
    )
        .bind(&like)
        .bind(&term)
        .bind(limit)
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
    let artist = sqlx::query_as::<_, ArtistRow>(&format!("SELECT {ARTIST_COLS} FROM artists WHERE id = $1"))
        .bind(artist_id)
        .fetch_optional(&st.pg)
        .await?
        .ok_or_else(|| AppError::not_found("artist not found"))?;

    let sc_accounts = sqlx::query_as::<_, ScAccountRow>(
        "SELECT sc_user_id, role, source, verified, notes FROM artist_sc_accounts \
         WHERE artist_id = $1 ORDER BY role, sc_user_id",
    )
        .bind(artist_id)
        .fetch_all(&st.pg)
        .await?;

    let track_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::int8 FROM track_artists WHERE artist_id = $1")
            .bind(artist_id)
            .fetch_one(&st.pg)
            .await?;
    let album_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::int8 FROM album_artists WHERE artist_id = $1")
            .bind(artist_id)
            .fetch_one(&st.pg)
            .await?;

    Ok(Json(ArtistDetail { artist, sc_accounts, track_count, album_count }))
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

    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM artists WHERE normalized_name = $1 AND merged_into IS NULL)",
    )
        .bind(&normalized)
        .fetch_one(&st.pg)
        .await?;
    if exists {
        return Err(AppError::bad_request("artist with this name already exists"));
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
    let name = body.name.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let normalized = name.as_deref().map(normalize_name);

    let row = sqlx::query_as::<_, ArtistRow>(&format!(
        "UPDATE artists SET \
            name = COALESCE($2, name), \
            normalized_name = COALESCE($3, normalized_name), \
            country = COALESCE($4, country), \
            bio = COALESCE($5, bio), \
            avatar_url = COALESCE($6, avatar_url), \
            sc_user_id = COALESCE($7, sc_user_id), \
            confidence = COALESCE($8, confidence), \
            updated_at = now() \
         WHERE id = $1 RETURNING {ARTIST_COLS}"
    ))
        .bind(artist_id)
        .bind(&name)
        .bind(&normalized)
        .bind(&body.country)
        .bind(&body.bio)
        .bind(&body.avatar_url)
        .bind(&body.sc_user_id)
        .bind(body.confidence)
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
    let like = q.q.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).map(|s| format!("%{s}%"));

    let rows = sqlx::query_as::<_, AlbumListRow>(
        "SELECT al.id, al.title, al.type AS type_, al.release_year, al.primary_artist_id, \
                a.name AS primary_artist_name, \
                (SELECT COUNT(*)::int8 FROM album_tracks t WHERE t.album_id = al.id) AS track_count \
         FROM albums al LEFT JOIN artists a ON a.id = al.primary_artist_id \
         WHERE ($1::text IS NULL OR al.title ILIKE $1) \
         ORDER BY al.title ASC LIMIT $2",
    )
        .bind(&like)
        .bind(limit)
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

const TRACK_SELECT: &str = "SELECT t.id, t.sc_track_id, t.title, t.metadata_artist, t.artwork_url, \
        t.primary_artist_id, a.name AS primary_artist_name, t.album_id, al.title AS album_title, \
        t.enrich_state, t.release_year \
     FROM tracks t \
     LEFT JOIN artists a ON a.id = t.primary_artist_id \
     LEFT JOIN albums al ON al.id = t.album_id";

#[tracing::instrument(skip_all)]
pub async fn tracks_search(
    _: AdminAuth,
    State(st): State<AppState>,
    Query(q): Query<TracksQuery>,
) -> AppResult<Json<Vec<TrackListRow>>> {
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let term = q.q.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let like = term.as_ref().map(|s| format!("%{s}%"));

    let rows = sqlx::query_as::<_, TrackListRow>(&format!(
        "{TRACK_SELECT} \
         WHERE ($1::text IS NULL OR t.title ILIKE $1 OR t.metadata_artist ILIKE $1 OR t.sc_track_id = $2) \
         ORDER BY t.sc_created_at DESC NULLS LAST LIMIT $3"
    ))
        .bind(&like)
        .bind(&term)
        .bind(limit)
        .fetch_all(&st.pg)
        .await?;
    Ok(Json(rows))
}

#[derive(Serialize, sqlx::FromRow)]
pub struct TrackCreditRow {
    pub artist_id: Uuid,
    pub name: Option<String>,
    pub role: String,
    pub position: i16,
    pub source: String,
}

#[derive(Serialize)]
pub struct TrackDetail {
    #[serde(flatten)]
    pub track: TrackListRow,
    pub credits: Vec<TrackCreditRow>,
}

#[tracing::instrument(skip_all)]
pub async fn track_detail(
    _: AdminAuth,
    State(st): State<AppState>,
    Path(track_id): Path<Uuid>,
) -> AppResult<Json<TrackDetail>> {
    let track = sqlx::query_as::<_, TrackListRow>(&format!("{TRACK_SELECT} WHERE t.id = $1"))
        .bind(track_id)
        .fetch_optional(&st.pg)
        .await?
        .ok_or_else(|| AppError::not_found("track not found"))?;

    let credits = sqlx::query_as::<_, TrackCreditRow>(
        "SELECT ta.artist_id, a.name, ta.role, ta.position, ta.source \
         FROM track_artists ta LEFT JOIN artists a ON a.id = ta.artist_id \
         WHERE ta.track_id = $1 ORDER BY ta.role, ta.position",
    )
        .bind(track_id)
        .fetch_all(&st.pg)
        .await?;

    Ok(Json(TrackDetail { track, credits }))
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
    let artist_ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM artists WHERE id = $1)")
        .bind(body.artist_id)
        .fetch_one(&st.pg)
        .await?;
    if !artist_ok {
        return Err(AppError::bad_request("artist not found"));
    }

    let mut tx = st.pg.begin().await?;
    let updated = sqlx::query("UPDATE tracks SET primary_artist_id = $1 WHERE id = $2")
        .bind(body.artist_id)
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::not_found("track not found"));
    }
    sqlx::query("DELETE FROM track_artists WHERE track_id = $1 AND role = 'primary'")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence) \
         VALUES ($1, $2, 'primary', 0, 'manual', 1.0) ON CONFLICT DO NOTHING",
    )
        .bind(track_id)
        .bind(body.artist_id)
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
        let album_ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM albums WHERE id = $1)")
            .bind(album_id)
            .fetch_one(&st.pg)
            .await?;
        if !album_ok {
            return Err(AppError::bad_request("album not found"));
        }
    }

    let mut tx = st.pg.begin().await?;
    let updated = sqlx::query("UPDATE tracks SET album_id = $1 WHERE id = $2")
        .bind(body.album_id)
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
    if updated.rows_affected() == 0 {
        return Err(AppError::not_found("track not found"));
    }
    sqlx::query("DELETE FROM album_tracks WHERE track_id = $1")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;
    if let Some(album_id) = body.album_id {
        sqlx::query(
            "INSERT INTO album_tracks (album_id, track_id, position) VALUES ($1, $2, NULL) \
             ON CONFLICT DO NOTHING",
        )
            .bind(album_id)
            .bind(track_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true, "album_id": body.album_id })))
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
        return Err(AppError::bad_request("role must be one of: primary, feature, remixer, producer"));
    }
    let artist_ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM artists WHERE id = $1)")
        .bind(body.artist_id)
        .fetch_one(&st.pg)
        .await?;
    if !artist_ok {
        return Err(AppError::bad_request("artist not found"));
    }

    let mut tx = st.pg.begin().await?;
    let track_ok: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM tracks WHERE id = $1)")
        .bind(track_id)
        .fetch_one(&mut *tx)
        .await?;
    if !track_ok {
        return Err(AppError::not_found("track not found"));
    }

    sqlx::query(
        "INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence) \
         VALUES ($1, $2, $3, COALESCE($4, 0), 'manual', 1.0) \
         ON CONFLICT (track_id, artist_id, role) \
         DO UPDATE SET position = EXCLUDED.position, source = 'manual', confidence = 1.0",
    )
        .bind(track_id)
        .bind(body.artist_id)
        .bind(&role)
        .bind(body.position)
        .execute(&mut *tx)
        .await?;

    if role == "primary" {
        sqlx::query("DELETE FROM track_artists WHERE track_id = $1 AND role = 'primary' AND artist_id <> $2")
            .bind(track_id)
            .bind(body.artist_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE tracks SET primary_artist_id = $1 WHERE id = $2")
            .bind(body.artist_id)
            .bind(track_id)
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
    let res = sqlx::query("DELETE FROM track_artists WHERE track_id = $1 AND artist_id = $2 AND role = $3")
        .bind(track_id)
        .bind(artist_id)
        .bind(&role)
        .execute(&mut *tx)
        .await?;
    if role == "primary" {
        sqlx::query("UPDATE tracks SET primary_artist_id = NULL WHERE id = $1 AND primary_artist_id = $2")
            .bind(track_id)
            .bind(artist_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(Json(serde_json::json!({ "ok": true, "removed": res.rows_affected() })))
}
