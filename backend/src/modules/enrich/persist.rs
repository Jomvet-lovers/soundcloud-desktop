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
        sqlx::query_file_scalar!("queries/enrich/persist/count_track_artists.sql", track_id)
            .fetch_one(&mut *tx)
            .await?;

    sqlx::query_file!("queries/enrich/persist/delete_track_artists.sql", track_id)
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
        sqlx::query_file!(
            "queries/enrich/persist/set_uploader_sc_user_id.sql",
            track_id,
            sc_id
        )
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
    sqlx::query_file!(
        "queries/enrich/persist/finalize_track.sql",
        track_id,
        primary_artist_id,
        album_id,
        res.isrc.as_deref(),
        canonical_id,
        cover_of_artist_id,
        source,
        confidence,
        upload_kind,
        res.release_date,
        res.release_year
    )
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
    let row: Option<f32> = sqlx::query_file_scalar!(
        "queries/enrich/persist/calibrated_confidence.sql",
        source,
        raw
    )
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row.map(|v| v.clamp(0.0, 1.0)).unwrap_or(raw))
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
    let exists: Option<String> = sqlx::query_file_scalar!(
        "queries/enrich/persist/sc_account_role.sql",
        artist_id,
        sc_user_id
    )
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
    let has_main: Option<i64> = sqlx::query_file_scalar!(
        "queries/enrich/persist/count_main_sc_accounts.sql",
        artist_id
    )
    .fetch_optional(&mut **tx)
    .await?;
    let role = match has_main {
        Some(n) if n == 0 && exact => "main",
        _ => "alt",
    };
    sqlx::query_file!(
        "queries/enrich/persist/insert_auto_match_account.sql",
        artist_id,
        sc_user_id,
        role
    )
    .execute(&mut **tx)
    .await?;
    sqlx::query_file!(
        "queries/enrich/persist/set_artist_sc_user_id.sql",
        artist_id,
        sc_user_id
    )
    .execute(&mut **tx)
    .await?;
    // Re-point this uploader's already-enriched tracks to the newly matched artist
    // (skip in-flight/locked). Jitter next_run_at so a prolific reupload channel
    // doesn't make its whole catalog claimable at once and swamp the enrich pool.
    sqlx::query_file!(
        "queries/enrich/persist/repoint_uploader_tracks.sql",
        sc_user_id,
        artist_id
    )
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
    let exists: Option<i32> = sqlx::query_file_scalar!(
        "queries/enrich/persist/sc_account_exists.sql",
        artist_id,
        sc_user_id
    )
    .fetch_optional(&mut **tx)
    .await?;
    if exists.is_some() {
        return Ok(());
    }
    let count: i64 = sqlx::query_file_scalar!(
        "queries/enrich/persist/count_reupload_tracks.sql",
        sc_user_id,
        artist_id
    )
    .fetch_one(&mut **tx)
    .await?;
    if count < REUPLOAD_THRESHOLD {
        return Ok(());
    }
    sqlx::query_file!(
        "queries/enrich/persist/insert_reupload_account.sql",
        artist_id,
        sc_user_id
    )
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
            let row = sqlx::query_file!(
                "queries/enrich/persist/sc_account_role_verified.sql",
                artist_id,
                sc_id
            )
            .fetch_optional(&mut **tx)
            .await?;
            if let Some(r) = row {
                return Ok(match (r.role.as_str(), r.verified) {
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
        let existing: Option<Uuid> =
            sqlx::query_file_scalar!("queries/enrich/persist/artist_by_mb_id.sql", mb_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(id) = existing {
            maybe_promote(tx, id, cand, source, confidence).await?;
            return resolve_merged(tx, id).await;
        }
    }

    if let Some(genius_id) = cand.genius_id.as_deref() {
        let existing: Option<Uuid> =
            sqlx::query_file_scalar!("queries/enrich/persist/artist_by_genius_id.sql", genius_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(id) = existing {
            maybe_promote(tx, id, cand, source, confidence).await?;
            return resolve_merged(tx, id).await;
        }
    }

    let existing: Option<Uuid> = sqlx::query_file_scalar!(
        "queries/enrich/persist/artist_by_normalized_name.sql",
        normalized
    )
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = existing {
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
    let row = sqlx::query_file!("queries/enrich/persist/artist_promote_row.sql", id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(r) = row else {
        return Ok(());
    };
    let (cur_source, cur_conf, cur_mb, cur_genius, cur_sc) = (
        r.source,
        r.confidence,
        r.mb_artist_id,
        r.genius_artist_id,
        r.sc_user_id,
    );
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
    sqlx::query_file!(
        "queries/enrich/persist/promote_artist.sql",
        id,
        mb_to_set.as_deref(),
        genius_to_set.as_deref(),
        sc_to_set.as_deref(),
        stronger,
        source.as_str(),
        confidence
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn resolve_merged(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> AppResult<Uuid> {
    let mut current = id;
    for _ in 0..4 {
        let next: Option<Option<Uuid>> =
            sqlx::query_file_scalar!("queries/enrich/persist/artist_merged_into.sql", current)
                .fetch_optional(&mut **tx)
                .await?;
        match next {
            Some(Some(parent)) => current = parent,
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
        sqlx::query_file!(
            "queries/enrich/persist/insert_track_artist.sql",
            track_id,
            id,
            role,
            pos as i16,
            source.as_str(),
            confidence
        )
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
        let existing: Option<Uuid> =
            sqlx::query_file_scalar!("queries/enrich/persist/album_by_mb_id.sql", mb_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(id) = existing {
            if let Some(cover) = album.cover_url.as_deref() {
                sqlx::query_file!(
                    "queries/enrich/persist/album_fill_cover_mb.sql",
                    id,
                    cover,
                    album.year
                )
                .execute(&mut **tx)
                .await?;
            }
            return Ok(id);
        }
    }
    if let Some(g_id) = album.genius_id.as_deref() {
        let existing: Option<Uuid> =
            sqlx::query_file_scalar!("queries/enrich/persist/album_by_genius_id.sql", g_id)
                .fetch_optional(&mut **tx)
                .await?;
        if let Some(id) = existing {
            sqlx::query_file!(
                "queries/enrich/persist/album_fill_cover_genius.sql",
                id,
                album.cover_url.as_deref(),
                album.year
            )
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
        sqlx::query_file!(
            "queries/enrich/persist/insert_album_artist.sql",
            inserted.0,
            pa_id
        )
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
    sqlx::query_file!(
        "queries/enrich/persist/insert_album_track.sql",
        album_id,
        track_id
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn resolve_canonical_for_isrc(
    tx: &mut Transaction<'_, Postgres>,
    track_id: Uuid,
    isrc: &str,
) -> AppResult<Uuid> {
    // Serialize canonical assignment per ISRC inside the tx so two concurrent
    // same-ISRC enrichments cannot each mint a different canonical id and split
    // the group. DB-only lock, released at commit; no .await on external work
    // is held under it.
    sqlx::query_file!("queries/enrich/persist/isrc_advisory_lock.sql", isrc)
        .execute(&mut **tx)
        .await?;
    let existing: Option<Uuid> = sqlx::query_file_scalar!(
        "queries/enrich/persist/existing_canonical_for_isrc.sql",
        isrc,
        track_id
    )
    .fetch_optional(&mut **tx)
    .await?;
    match existing {
        Some(cid) => Ok(cid),
        None => {
            let new_id = Uuid::new_v4();
            sqlx::query_file!(
                "queries/enrich/persist/assign_canonical_for_isrc.sql",
                new_id,
                isrc
            )
            .execute(&mut **tx)
            .await?;
            Ok(new_id)
        }
    }
}
