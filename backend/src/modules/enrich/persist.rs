use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::enrich::normalize::{clean_artist_name, normalize_name, normalize_title};
use crate::modules::enrich::resolver::{
    AlbumCandidate, ArtistCandidate, ResolveResult, ResolveSource,
};

pub struct PersistOutcome {
    pub primary_artist_id: Option<Uuid>,
    pub album_id: Option<Uuid>,
    pub coplay_dirty: bool,
}

pub async fn apply(
    pg: &PgPool,
    track_id: Uuid,
    res: &ResolveResult,
    uploader_sc_user_id: Option<&str>,
    uploader_username: Option<&str>,
) -> AppResult<PersistOutcome> {
    let mut tx = pg.begin().await?;

    let primary_ids = upsert_artists(&mut tx, &res.primary, res.source, res.confidence).await?;
    let featured_ids = upsert_artists(&mut tx, &res.featured, res.source, res.confidence).await?;
    let producer_ids =
        upsert_artists(&mut tx, &res.producers, ResolveSource::Heuristic, 0.3).await?;
    let remixer_ids = upsert_artists(&mut tx, &res.remixers, ResolveSource::Heuristic, 0.4).await?;

    let prior_count: i64 =
        sqlx::query_scalar("SELECT COUNT(*)::int8 FROM track_artists WHERE track_id = $1")
            .bind(track_id)
            .fetch_one(&mut *tx)
            .await?;

    sqlx::query("DELETE FROM track_artists WHERE track_id = $1")
        .bind(track_id)
        .execute(&mut *tx)
        .await?;

    let mut all_artist_ids: Vec<Uuid> = Vec::new();
    // Для cover'а track_artists НЕ заполняем — primary это original
    // (uploader != original), featured/prod/remix относятся к оригиналу,
    // не к этой записи uploader'а. Original связан через cover_of_artist_id.
    if !res.is_cover {
        insert_track_artists(
            &mut tx,
            track_id,
            &primary_ids,
            "primary",
            res.source,
            res.confidence,
            &mut all_artist_ids,
        )
        .await?;
        insert_track_artists(
            &mut tx,
            track_id,
            &featured_ids,
            "featured",
            res.source,
            res.confidence,
            &mut all_artist_ids,
        )
        .await?;
        insert_track_artists(
            &mut tx,
            track_id,
            &producer_ids,
            "producer",
            ResolveSource::Heuristic,
            0.3,
            &mut all_artist_ids,
        )
        .await?;
        insert_track_artists(
            &mut tx,
            track_id,
            &remixer_ids,
            "remixer",
            ResolveSource::Heuristic,
            0.4,
            &mut all_artist_ids,
        )
        .await?;
    } else {
        // suppress unused warning
        let _ = (&featured_ids, &producer_ids, &remixer_ids);
    }

    // Кавер: найденный по title в MB/Genius артист — это ОРИГИНАЛ. Кладём
    // в cover_of_artist_id; primary_artist_id остаётся NULL (uploader не
    // равен оригиналу). track_artists для cover'а тоже не нужны (мы выше уже
    // удалили — INSERT'ов не делаем).
    let (primary_artist_id, cover_of_artist_id) = if res.is_cover {
        (None, primary_ids.first().copied())
    } else {
        (primary_ids.first().copied(), None)
    };

    let album_id = if let Some(album) = res.album.as_ref() {
        Some(upsert_album(&mut tx, album, res.source, res.confidence).await?)
    } else {
        None
    };

    if let (Some(album_id), Some(_)) = (album_id, primary_artist_id) {
        link_album_track(&mut tx, album_id, track_id).await?;
    }

    let canonical_id = match res.isrc.as_deref() {
        Some(isrc) if !isrc.is_empty() => {
            Some(resolve_canonical_for_isrc(&mut tx, track_id, isrc).await?)
        }
        _ => None,
    };

    if let (Some(artist_id), Some(sc_id)) = (primary_artist_id, uploader_sc_user_id) {
        // Сохраняем uploader_sc_user_id в tracks, чтобы reupload-pattern
        // увидел текущий трек в счётчике сразу.
        sqlx::query(
            "UPDATE tracks SET uploader_sc_user_id = COALESCE(uploader_sc_user_id, $2)
             WHERE id = $1",
        )
        .bind(track_id)
        .bind(sc_id)
        .execute(&mut *tx)
        .await?;

        let primary_name = res.primary.first().map(|c| c.name.as_str()).unwrap_or("");
        maybe_auto_attach_sc_account(
            &mut tx,
            artist_id,
            sc_id,
            uploader_username.unwrap_or(""),
            primary_name,
        )
        .await?;

        maybe_attach_reupload_account(&mut tx, artist_id, sc_id).await?;
    }

    let upload_kind = if res.is_cover {
        "cover"
    } else {
        compute_upload_kind(&mut tx, primary_artist_id, uploader_sc_user_id, res.source).await?
    };

    let source = res.source.as_str();
    let confidence = calibrate_confidence(&mut tx, source, res.confidence).await?;
    // release_date пишем по приоритету:
    //   1. свежий Genius song/album.release_date — он знает реальный релиз,
    //   2. ранее сохранённое значение — не теряем дату, найденную прошлым enrich'ем,
    //   3. fallback на sc_created_at::date — дата заливки на SoundCloud.
    // release_year — синхронно через тот же приоритет. Используется в sort
    // "новые" и в group-by-year на странице артиста.
    sqlx::query(
        "UPDATE tracks
         SET primary_artist_id = $2,
             album_id = $3,
             isrc = $4,
             canonical_track_id = COALESCE($5, canonical_track_id),
             cover_of_artist_id = $6,
             release_date = COALESCE($10, release_date, sc_created_at::date),
             release_year = COALESCE(
                 $11,
                 EXTRACT(YEAR FROM $10::date)::smallint,
                 release_year,
                 EXTRACT(YEAR FROM sc_created_at)::smallint
             ),
             enrich_state = 'done',
             enrich_source = $7,
             enrich_confidence = $8,
             enrich_attempts = enrich_attempts + 1,
             enriched_at = now(),
             upload_kind = $9
         WHERE id = $1",
    )
    .bind(track_id)
    .bind(primary_artist_id)
    .bind(album_id)
    .bind(res.isrc.as_deref())
    .bind(canonical_id)
    .bind(cover_of_artist_id)
    .bind(source)
    .bind(confidence)
    .bind(upload_kind)
    .bind(res.release_date)
    .bind(res.release_year)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(PersistOutcome {
        primary_artist_id,
        album_id,
        coplay_dirty: prior_count == 0 && all_artist_ids.len() >= 2,
    })
}

async fn calibrate_confidence(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    raw: f32,
) -> AppResult<f32> {
    let row: Option<(f32,)> = sqlx::query_as(
        "SELECT calibrated FROM enrich_calibration
         WHERE source = $1 AND raw_bin_low <= $2 AND $2 < raw_bin_high
         ORDER BY raw_bin_low DESC
         LIMIT 1",
    )
    .bind(source)
    .bind(raw)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|(v,)| v.clamp(0.0, 1.0)).unwrap_or(raw))
}

async fn maybe_auto_attach_sc_account(
    tx: &mut Transaction<'_, Postgres>,
    artist_id: Uuid,
    sc_user_id: &str,
    uploader_username: &str,
    artist_name: &str,
) -> AppResult<()> {
    if sc_user_id.is_empty() || uploader_username.is_empty() || artist_name.is_empty() {
        return Ok(());
    }
    let exists: Option<(String,)> = sqlx::query_as(
        "SELECT role FROM artist_sc_accounts WHERE artist_id = $1 AND sc_user_id = $2",
    )
    .bind(artist_id)
    .bind(sc_user_id)
    .fetch_optional(&mut **tx)
    .await?;
    if exists.is_some() {
        return Ok(());
    }
    let un = normalize_name(uploader_username);
    let an = normalize_name(artist_name);
    if un.is_empty() || an.is_empty() {
        return Ok(());
    }
    let exact = un == an;
    let strong_substring = un.len() >= 4 && an.len() >= 4 && (un.contains(&an) || an.contains(&un));
    if !exact && !strong_substring {
        return Ok(());
    }
    let has_main: Option<(i64,)> = sqlx::query_as(
        "SELECT COUNT(*)::int8 FROM artist_sc_accounts WHERE artist_id = $1 AND role = 'main'",
    )
    .bind(artist_id)
    .fetch_optional(&mut **tx)
    .await?;
    let role = match has_main {
        Some((n,)) if n == 0 && exact => "main",
        _ => "alt",
    };
    sqlx::query(
        "INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified)
         VALUES ($1, $2, $3, 'auto_match', false)
         ON CONFLICT (artist_id, sc_user_id) DO NOTHING",
    )
    .bind(artist_id)
    .bind(sc_user_id)
    .bind(role)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "UPDATE artists SET sc_user_id = COALESCE(sc_user_id, $2), updated_at = now()
         WHERE id = $1",
    )
    .bind(artist_id)
    .bind(sc_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Если у одного SC user'а уже есть >= REUPLOAD_THRESHOLD треков, у которых
/// primary_artist = этот же артист — это явный перезалив-канал. Привязываем
/// его как `alt` (verified=false), чтобы sc_account_scan мог использовать
/// этот аккаунт при поиске остальных треков артиста.
const REUPLOAD_THRESHOLD: i64 = 3;

async fn maybe_attach_reupload_account(
    tx: &mut Transaction<'_, Postgres>,
    artist_id: Uuid,
    sc_user_id: &str,
) -> AppResult<()> {
    if sc_user_id.is_empty() {
        return Ok(());
    }
    let exists: Option<(i32,)> =
        sqlx::query_as("SELECT 1 FROM artist_sc_accounts WHERE artist_id = $1 AND sc_user_id = $2")
            .bind(artist_id)
            .bind(sc_user_id)
            .fetch_optional(&mut **tx)
            .await?;
    if exists.is_some() {
        return Ok(());
    }
    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(DISTINCT it.id)::int8
         FROM tracks it
         JOIN track_artists ta ON ta.track_id = it.id AND ta.role = 'primary'
         WHERE it.uploader_sc_user_id = $1 AND ta.artist_id = $2",
    )
    .bind(sc_user_id)
    .bind(artist_id)
    .fetch_one(&mut **tx)
    .await?;
    if count.0 < REUPLOAD_THRESHOLD {
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO artist_sc_accounts (artist_id, sc_user_id, role, source, verified)
         VALUES ($1, $2, 'alt', 'reupload_pattern', false)
         ON CONFLICT (artist_id, sc_user_id) DO NOTHING",
    )
    .bind(artist_id)
    .bind(sc_user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn compute_upload_kind(
    tx: &mut Transaction<'_, Postgres>,
    primary_artist_id: Option<Uuid>,
    uploader_sc_user_id: Option<&str>,
    source: ResolveSource,
) -> AppResult<&'static str> {
    let Some(artist_id) = primary_artist_id else {
        return Ok("unknown");
    };
    if let Some(sc_id) = uploader_sc_user_id {
        if !sc_id.is_empty() {
            let row: Option<(String, bool)> = sqlx::query_as(
                "SELECT role, verified FROM artist_sc_accounts WHERE artist_id = $1 AND sc_user_id = $2",
            )
            .bind(artist_id)
            .bind(sc_id)
            .fetch_optional(&mut **tx)
            .await?;
            if let Some((role, verified)) = row {
                return Ok(match (role.as_str(), verified) {
                    ("main", true) => "original",
                    ("demo", _) => "demo",
                    ("main", false) => "alt",
                    ("alt", _) => "alt",
                    _ => "unknown",
                });
            }
        }
    }
    let verified_source = matches!(
        source,
        ResolveSource::Isrc | ResolveSource::Mb | ResolveSource::Genius | ResolveSource::ScVerified
    );
    Ok(if verified_source {
        "reupload"
    } else {
        "unknown"
    })
}

pub async fn mark_failed(pg: &PgPool, track_id: Uuid, error: &str) -> AppResult<()> {
    let truncated: String = error.chars().take(200).collect();
    sqlx::query(
        "UPDATE tracks
         SET enrich_state = 'failed',
             enrich_attempts = enrich_attempts + 1,
             enriched_at = now(),
             enrich_source = $2
         WHERE id = $1",
    )
    .bind(track_id)
    .bind(truncated)
    .execute(pg)
    .await?;
    Ok(())
}

async fn upsert_artists(
    tx: &mut Transaction<'_, Postgres>,
    candidates: &[ArtistCandidate],
    source: ResolveSource,
    confidence: f32,
) -> AppResult<Vec<Uuid>> {
    let mut ids = Vec::with_capacity(candidates.len());
    for c in candidates {
        let cleaned = clean_artist_name(&c.name);
        if cleaned.is_empty() {
            continue;
        }
        let normalized = normalize_name(&cleaned);
        if normalized.is_empty() {
            continue;
        }
        let id = upsert_one_artist(tx, &cleaned, &normalized, c, source, confidence).await?;
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

async fn upsert_one_artist(
    tx: &mut Transaction<'_, Postgres>,
    name: &str,
    normalized: &str,
    cand: &ArtistCandidate,
    source: ResolveSource,
    confidence: f32,
) -> AppResult<Uuid> {
    if let Some(mb_id) = cand.mb_id.as_deref() {
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM artists WHERE mb_artist_id = $1 LIMIT 1")
                .bind(mb_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some((id,)) = existing {
            maybe_promote(tx, id, cand, source, confidence).await?;
            return resolve_merged(tx, id).await;
        }
    }

    if let Some(genius_id) = cand.genius_id.as_deref() {
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM artists WHERE genius_artist_id = $1 LIMIT 1")
                .bind(genius_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some((id,)) = existing {
            maybe_promote(tx, id, cand, source, confidence).await?;
            return resolve_merged(tx, id).await;
        }
    }

    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT id FROM artists WHERE normalized_name = $1 AND merged_into IS NULL LIMIT 1",
    )
    .bind(normalized)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some((id,)) = existing {
        maybe_promote(tx, id, cand, source, confidence).await?;
        return resolve_merged(tx, id).await;
    }

    let inserted: (Uuid,) = sqlx::query_as(
        "INSERT INTO artists (name, normalized_name, mb_artist_id, genius_artist_id, sc_user_id, source, confidence)
         VALUES ($1, $2, $3, $4, $5, $6, $7)
         RETURNING id",
    )
    .bind(name)
    .bind(normalized)
    .bind(cand.mb_id.as_deref())
    .bind(cand.genius_id.as_deref())
    .bind(cand.sc_user_id.as_deref())
    .bind(source.as_str())
    .bind(confidence)
    .fetch_one(&mut **tx)
    .await?;
    Ok(inserted.0)
}

async fn maybe_promote(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    cand: &ArtistCandidate,
    source: ResolveSource,
    confidence: f32,
) -> AppResult<()> {
    let row: Option<(String, f32, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT source, confidence, mb_artist_id, genius_artist_id, sc_user_id FROM artists WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    let Some((cur_source, cur_conf, cur_mb, cur_genius, cur_sc)) = row else {
        return Ok(());
    };
    let new_priority = source.priority();
    let cur_priority = ResolveSource::priority_of(&cur_source);
    let stronger = new_priority > cur_priority
        || (new_priority == cur_priority && confidence > cur_conf + 0.05);
    let mb_to_set = cand.mb_id.clone().or(cur_mb);
    let genius_to_set = cand.genius_id.clone().or(cur_genius);
    let sc_to_set = cand.sc_user_id.clone().or(cur_sc);
    if !stronger && mb_to_set.is_none() && genius_to_set.is_none() && sc_to_set.is_none() {
        return Ok(());
    }
    sqlx::query(
        "UPDATE artists
         SET mb_artist_id     = COALESCE(mb_artist_id,     $2),
             genius_artist_id = COALESCE(genius_artist_id, $3),
             sc_user_id       = COALESCE(sc_user_id,       $4),
             source           = CASE WHEN $5 THEN $6 ELSE source END,
             confidence       = CASE WHEN $5 THEN $7 ELSE confidence END,
             updated_at       = now()
         WHERE id = $1",
    )
    .bind(id)
    .bind(mb_to_set.as_deref())
    .bind(genius_to_set.as_deref())
    .bind(sc_to_set.as_deref())
    .bind(stronger)
    .bind(source.as_str())
    .bind(confidence)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn resolve_merged(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> AppResult<Uuid> {
    let mut current = id;
    for _ in 0..4 {
        let next: Option<(Option<Uuid>,)> =
            sqlx::query_as("SELECT merged_into FROM artists WHERE id = $1")
                .bind(current)
                .fetch_optional(&mut **tx)
                .await?;
        match next {
            Some((Some(parent),)) => current = parent,
            _ => break,
        }
    }
    Ok(current)
}

async fn insert_track_artists(
    tx: &mut Transaction<'_, Postgres>,
    track_id: Uuid,
    artist_ids: &[Uuid],
    role: &str,
    source: ResolveSource,
    confidence: f32,
    accum: &mut Vec<Uuid>,
) -> AppResult<()> {
    for (pos, id) in artist_ids.iter().enumerate() {
        sqlx::query(
            "INSERT INTO track_artists (track_id, artist_id, role, position, source, confidence)
             VALUES ($1, $2, $3, $4, $5, $6)
             ON CONFLICT (track_id, artist_id, role) DO UPDATE
             SET position = EXCLUDED.position,
                 source = EXCLUDED.source,
                 confidence = EXCLUDED.confidence",
        )
        .bind(track_id)
        .bind(id)
        .bind(role)
        .bind(pos as i16)
        .bind(source.as_str())
        .bind(confidence)
        .execute(&mut **tx)
        .await?;
        if !accum.contains(id) {
            accum.push(*id);
        }
    }
    Ok(())
}

async fn upsert_album(
    tx: &mut Transaction<'_, Postgres>,
    album: &AlbumCandidate,
    source: ResolveSource,
    confidence: f32,
) -> AppResult<Uuid> {
    if let Some(mb_id) = album.mb_id.as_deref() {
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM albums WHERE mb_release_id = $1 LIMIT 1")
                .bind(mb_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some((id,)) = existing {
            if let Some(cover) = album.cover_url.as_deref() {
                sqlx::query(
                    "UPDATE albums SET cover_url = COALESCE(cover_url, $2),
                                       release_year = COALESCE(release_year, $3),
                                       updated_at = now()
                     WHERE id = $1",
                )
                .bind(id)
                .bind(cover)
                .bind(album.year)
                .execute(&mut **tx)
                .await?;
            }
            return Ok(id);
        }
    }
    if let Some(g_id) = album.genius_id.as_deref() {
        let existing: Option<(Uuid,)> =
            sqlx::query_as("SELECT id FROM albums WHERE genius_album_id = $1 LIMIT 1")
                .bind(g_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some((id,)) = existing {
            sqlx::query(
                "UPDATE albums SET cover_url = COALESCE(cover_url, $2),
                                   release_year = COALESCE(release_year, $3),
                                   updated_at = now()
                 WHERE id = $1",
            )
            .bind(id)
            .bind(album.cover_url.as_deref())
            .bind(album.year)
            .execute(&mut **tx)
            .await?;
            return Ok(id);
        }
    }

    let primary_artist_id = if let Some(pa) = album.primary_artist.as_ref() {
        let n = normalize_name(&pa.name);
        if n.is_empty() {
            None
        } else {
            let id = upsert_one_artist(tx, pa.name.trim(), &n, pa, source, confidence).await?;
            Some(id)
        }
    } else {
        None
    };

    let normalized_title = normalize_title(&album.title);
    let kind = match album.release_type.as_deref() {
        Some("EP") => "ep",
        Some("Single") => "single",
        Some("Compilation") => "compilation",
        _ => "album",
    };

    let inserted: (Uuid,) = sqlx::query_as(
        "INSERT INTO albums (title, normalized_title, primary_artist_id, type, release_year, mb_release_id, genius_album_id, cover_url, source, confidence)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
         RETURNING id",
    )
    .bind(album.title.trim())
    .bind(&normalized_title)
    .bind(primary_artist_id)
    .bind(kind)
    .bind(album.year)
    .bind(album.mb_id.as_deref())
    .bind(album.genius_id.as_deref())
    .bind(album.cover_url.as_deref())
    .bind(source.as_str())
    .bind(confidence)
    .fetch_one(&mut **tx)
    .await?;

    if let Some(pa_id) = primary_artist_id {
        sqlx::query(
            "INSERT INTO album_artists (album_id, artist_id, role)
             VALUES ($1, $2, 'primary')
             ON CONFLICT DO NOTHING",
        )
        .bind(inserted.0)
        .bind(pa_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(inserted.0)
}

async fn link_album_track(
    tx: &mut Transaction<'_, Postgres>,
    album_id: Uuid,
    track_id: Uuid,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO album_tracks (album_id, track_id) VALUES ($1, $2)
         ON CONFLICT DO NOTHING",
    )
    .bind(album_id)
    .bind(track_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn resolve_canonical_for_isrc(
    tx: &mut Transaction<'_, Postgres>,
    track_id: Uuid,
    isrc: &str,
) -> AppResult<Uuid> {
    let existing: Option<(Uuid,)> = sqlx::query_as(
        "SELECT canonical_track_id FROM tracks
         WHERE isrc = $1 AND canonical_track_id IS NOT NULL AND id <> $2
         LIMIT 1",
    )
    .bind(isrc)
    .bind(track_id)
    .fetch_optional(&mut **tx)
    .await?;
    match existing {
        Some((cid,)) => Ok(cid),
        None => {
            let new_id = Uuid::new_v4();
            sqlx::query(
                "UPDATE tracks
                 SET canonical_track_id = $1
                 WHERE isrc = $2 AND canonical_track_id IS NULL",
            )
            .bind(new_id)
            .bind(isrc)
            .execute(&mut **tx)
            .await?;
            Ok(new_id)
        }
    }
}
