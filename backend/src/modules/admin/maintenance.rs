//! Разовые фоновые чистки каталога после смены алгоритмов имён:
//!
//!   * перенормализация `artists.normalized_name` под новый fold
//!     (ᴍᴏɴᴀʀᴄʜ → monarch); коллизия ключа = тот же артист, записанный
//!     по-разному — помечаем `merged_into` на владельца ключа;
//!   * репоинт ссылок со слитых артистов на холдера (track_artists,
//!     tracks.primary/cover, albums, album_artists) — иначе клик по автору
//!     ведёт на страницу merged-артиста = 404;
//!   * перенормализация `title_normalized` у tracks/playlists и
//!     `normalized_title` у albums (тот же fold, что у имён);
//!   * расшивка литеральных `\uXXXX` в `tracks.metadata_artist`.
//!
//! POST /admin/maintenance/renormalize — идемпотентно, повторный вызов на
//! уже чистых данных ничего не меняет. Работает батчами в фоне, прогресс в логе.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::modules::enrich::artist_names::{compact_key, unescape_json_unicode};
use crate::modules::enrich::mb::MbClient;
use crate::modules::enrich::normalize::{normalize_name, normalize_title};
use crate::state::AppState;

static RENORMALIZE_RUNNING: AtomicBool = AtomicBool::new(false);
static MB_NAMES_RUNNING: AtomicBool = AtomicBool::new(false);

const BATCH: i64 = 5_000;

/// Снимает флаг даже если таск запаниковал — иначе ручка навсегда
/// отвечает "already running" до рестарта.
struct FlagGuard(&'static AtomicBool);

impl Drop for FlagGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[tracing::instrument(skip_all)]
pub async fn renormalize(_: AdminAuth, State(st): State<AppState>) -> AppResult<Json<Value>> {
    if RENORMALIZE_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(Json(
            json!({ "started": false, "reason": "already running" }),
        ));
    }
    let pg = st.pg.clone();
    tokio::spawn(async move {
        let _guard = FlagGuard(&RENORMALIZE_RUNNING);
        if let Err(e) = run(&pg).await {
            warn!(error = %e, "maintenance renormalize failed");
        }
    });
    Ok(Json(json!({ "started": true })))
}

/// POST /admin/maintenance/mb-artist-names — сверка имён mb-артистов с именем
/// СУЩНОСТИ в MusicBrainz. Кредит-алиас с релиза ("SID" у SIDODJI DUBOSHIT)
/// минтил артиста-двойника; чиним: алиас-ряд переименовываем в имя сущности,
/// при коллизии ключа — сливаем в холдера с репоинтом ссылок. Идёт через
/// MB-throttle (~1.1с/артист), часы фоном; идемпотентно.
#[tracing::instrument(skip_all)]
pub async fn mb_artist_names(_: AdminAuth, State(st): State<AppState>) -> AppResult<Json<Value>> {
    if MB_NAMES_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(Json(
            json!({ "started": false, "reason": "already running" }),
        ));
    }
    let pg = st.pg.clone();
    let mb = st.enrich.mb();
    tokio::spawn(async move {
        let _guard = FlagGuard(&MB_NAMES_RUNNING);
        if let Err(e) = run_mb_names(&pg, &mb).await {
            warn!(error = %e, "maintenance mb-artist-names failed");
        }
    });
    Ok(Json(json!({ "started": true })))
}

async fn run(pg: &PgPool) -> AppResult<()> {
    renormalize_artists(pg).await?;
    repoint_merged_artists(pg).await?;
    canonicalize_credit_roles(pg).await?;
    renormalize_track_titles(pg).await?;
    renormalize_album_titles(pg).await?;
    renormalize_playlist_titles(pg).await?;
    unescape_track_meta(pg).await?;
    Ok(())
}

async fn run_mb_names(pg: &PgPool, mb: &Arc<MbClient>) -> AppResult<()> {
    let mut last = Uuid::nil();
    let (mut scanned, mut renamed, mut merged, mut variants, mut missed) =
        (0u64, 0u64, 0u64, 0u64, 0u64);
    loop {
        let rows = sqlx::query_file!("queries/admin/maintenance/artists_mb_scan.sql", last, BATCH)
            .fetch_all(pg)
            .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.id;
        scanned += rows.len() as u64;

        for r in &rows {
            // lookup_artist глотает транспортные ошибки в None — пропуск
            // безопасен, проход идемпотентен и перезапускаем.
            let Some(details) = mb.lookup_artist(&r.mb_artist_id).await? else {
                missed += 1;
                continue;
            };
            let Some(entity) = details.name else {
                missed += 1;
                continue;
            };
            let fresh = normalize_name(&entity);
            if fresh.is_empty() || fresh == r.normalized_name {
                continue;
            }
            // Алиас-класс: хранимое имя — кусок имени сущности ("SID" ⊂
            // "SIDODJI DUBOSHIT"). Иные расхождения (транслит/вариант записи:
            // "Лёд 9" vs "Lyod 9") не трогаем — наше имя может быть лучше.
            let stored_c = compact_key(&r.name);
            let entity_c = compact_key(&entity);
            if stored_c.is_empty() || !entity_c.contains(&stored_c) {
                variants += 1;
                debug!(stored = %r.name, entity = %entity, "mb name variant skipped");
                continue;
            }
            let holder: Option<Uuid> = sqlx::query_file_scalar!(
                "queries/admin/maintenance/artist_normalized_holder.sql",
                fresh,
                r.id
            )
            .fetch_optional(pg)
            .await?;
            match holder {
                Some(h) => {
                    merge_alias_into(pg, r.id, &r.mb_artist_id, r.genius_artist_id.as_deref(), h)
                        .await?;
                    merged += 1;
                    info!(alias = %r.name, entity = %entity, holder = %h, "mb alias merged");
                }
                None => {
                    let set = sqlx::query_file!(
                        "queries/admin/maintenance/artist_set_name.sql",
                        r.id,
                        &entity,
                        fresh
                    )
                    .execute(pg)
                    .await;
                    match set {
                        Ok(_) => {
                            renamed += 1;
                            info!(alias = %r.name, entity = %entity, "mb alias renamed");
                        }
                        Err(e) if is_unique_violation(&e) => {
                            // Гонка за ключ — холдер появился, сливаем.
                            let h: Option<Uuid> = sqlx::query_file_scalar!(
                                "queries/admin/maintenance/artist_normalized_holder.sql",
                                fresh,
                                r.id
                            )
                            .fetch_optional(pg)
                            .await?;
                            if let Some(h) = h {
                                merge_alias_into(
                                    pg,
                                    r.id,
                                    &r.mb_artist_id,
                                    r.genius_artist_id.as_deref(),
                                    h,
                                )
                                .await?;
                                merged += 1;
                            }
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
            }
        }
        info!(
            scanned,
            renamed, merged, variants, missed, "maintenance: mb artist names progress"
        );
    }
    info!(
        scanned,
        renamed, merged, variants, missed, "maintenance: mb artist names done"
    );
    Ok(())
}

/// Алиас-ряд сливается в холдера: внешние id переезжают (сначала очистка —
/// уникальные индексы), ссылки репоинтятся.
async fn merge_alias_into(
    pg: &PgPool,
    alias_id: Uuid,
    mb_id: &str,
    genius_id: Option<&str>,
    holder: Uuid,
) -> AppResult<()> {
    sqlx::query_file!(
        "queries/admin/maintenance/artist_mark_merged_clear_ids.sql",
        alias_id,
        holder
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/artist_fill_external_ids.sql",
        holder,
        mb_id,
        genius_id
    )
    .execute(pg)
    .await?;
    repoint_references(pg, alias_id, holder).await?;
    Ok(())
}

/// Легаси-роль 'feature' (старая админка) → канон 'featured', который знают
/// persist/DTO/фронт.
async fn canonicalize_credit_roles(pg: &PgPool) -> AppResult<()> {
    sqlx::query_file!("queries/admin/maintenance/role_feature_dedup.sql")
        .execute(pg)
        .await?;
    let updated = sqlx::query_file!("queries/admin/maintenance/role_feature_update.sql")
        .execute(pg)
        .await?;
    info!(
        updated = updated.rows_affected(),
        "maintenance: credit roles canonicalized"
    );
    Ok(())
}

async fn renormalize_artists(pg: &PgPool) -> AppResult<()> {
    let mut last = Uuid::nil();
    let (mut scanned, mut updated, mut merged) = (0u64, 0u64, 0u64);
    loop {
        let rows = sqlx::query_file!("queries/admin/maintenance/artists_scan.sql", last, BATCH)
            .fetch_all(pg)
            .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.id;
        scanned += rows.len() as u64;

        for r in &rows {
            let fresh = normalize_name(&r.name);
            if fresh.is_empty() || fresh == r.normalized_name {
                continue;
            }
            let set = sqlx::query_file!(
                "queries/admin/maintenance/artist_set_normalized.sql",
                r.id,
                fresh
            )
            .execute(pg)
            .await;
            match set {
                Ok(_) => updated += 1,
                Err(e) if is_unique_violation(&e) => {
                    // Ключ уже занят: это тот же артист в другом написании.
                    // Помечаем merged_into — новые апсерты пойдут во владельца,
                    // существующие ссылки репоинтит repoint_merged_artists.
                    let holder: Option<Uuid> = sqlx::query_file_scalar!(
                        "queries/admin/maintenance/artist_normalized_holder.sql",
                        fresh,
                        r.id
                    )
                    .fetch_optional(pg)
                    .await?;
                    if let Some(holder) = holder {
                        sqlx::query_file!(
                            "queries/admin/maintenance/artist_mark_merged.sql",
                            r.id,
                            holder
                        )
                        .execute(pg)
                        .await?;
                        merged += 1;
                    }
                }
                Err(e) => return Err(e.into()),
            }
        }
        info!(
            scanned,
            updated, merged, "maintenance: artists renormalize progress"
        );
    }
    info!(
        scanned,
        updated, merged, "maintenance: artists renormalize done"
    );
    Ok(())
}

/// Слитые артисты не должны оставаться в ссылках: страница merged-артиста
/// отвечает 404 (`detail_artist.sql` фильтрует `merged_into IS NULL`), а его
/// треки не видны на странице холдера.
async fn repoint_merged_artists(pg: &PgPool) -> AppResult<()> {
    let mut last = Uuid::nil();
    let mut repointed = 0u64;
    loop {
        let rows = sqlx::query_file!(
            "queries/admin/maintenance/merged_artists_scan.sql",
            last,
            BATCH
        )
        .fetch_all(pg)
        .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.id;

        for r in &rows {
            let holder = resolve_merge_root(pg, r.merged_into).await?;
            repoint_references(pg, r.id, holder).await?;
            repointed += 1;
        }
        info!(repointed, "maintenance: merged artists repoint progress");
    }
    info!(repointed, "maintenance: merged artists repoint done");
    Ok(())
}

/// merged_into может образовать цепочку (A→B→C) — ссылки ведём в корень.
async fn resolve_merge_root(pg: &PgPool, id: Uuid) -> AppResult<Uuid> {
    let mut current = id;
    for _ in 0..4 {
        let next: Option<Option<Uuid>> =
            sqlx::query_file_scalar!("queries/enrich/persist/artist_merged_into.sql", current)
                .fetch_optional(pg)
                .await?;
        match next {
            Some(Some(parent)) => current = parent,
            _ => break,
        }
    }
    Ok(current)
}

async fn repoint_references(pg: &PgPool, from: Uuid, to: Uuid) -> AppResult<()> {
    if from == to {
        return Ok(());
    }
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_track_artists_dedup.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_track_artists.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_tracks_primary.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_tracks_cover.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_albums_primary.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_album_artists_dedup.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/admin/maintenance/merge_repoint_album_artists.sql",
        from,
        to
    )
    .execute(pg)
    .await?;
    Ok(())
}

async fn renormalize_track_titles(pg: &PgPool) -> AppResult<()> {
    let mut last = Uuid::nil();
    let (mut scanned, mut updated) = (0u64, 0u64);
    loop {
        let rows = sqlx::query_file!(
            "queries/admin/maintenance/tracks_title_scan.sql",
            last,
            BATCH
        )
        .fetch_all(pg)
        .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.id;
        scanned += rows.len() as u64;

        for r in &rows {
            let fresh = normalize_title(&r.title);
            if fresh != r.title_normalized {
                sqlx::query_file!(
                    "queries/admin/maintenance/track_set_title_norm.sql",
                    r.id,
                    fresh
                )
                .execute(pg)
                .await?;
                updated += 1;
            }
        }
        if scanned % 100_000 < BATCH as u64 {
            info!(scanned, updated, "maintenance: track titles progress");
        }
    }
    info!(scanned, updated, "maintenance: track titles done");
    Ok(())
}

async fn renormalize_album_titles(pg: &PgPool) -> AppResult<()> {
    let mut last = Uuid::nil();
    let (mut scanned, mut updated) = (0u64, 0u64);
    loop {
        let rows = sqlx::query_file!(
            "queries/admin/maintenance/albums_title_scan.sql",
            last,
            BATCH
        )
        .fetch_all(pg)
        .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.id;
        scanned += rows.len() as u64;

        for r in &rows {
            let fresh = normalize_title(&r.title);
            if fresh != r.normalized_title {
                sqlx::query_file!(
                    "queries/admin/maintenance/album_set_title_norm.sql",
                    r.id,
                    fresh
                )
                .execute(pg)
                .await?;
                updated += 1;
            }
        }
    }
    info!(scanned, updated, "maintenance: album titles done");
    Ok(())
}

async fn renormalize_playlist_titles(pg: &PgPool) -> AppResult<()> {
    let mut last = String::new();
    let (mut scanned, mut updated) = (0u64, 0u64);
    loop {
        let rows = sqlx::query_file!(
            "queries/admin/maintenance/playlists_title_scan.sql",
            &last,
            BATCH
        )
        .fetch_all(pg)
        .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.urn.clone();
        scanned += rows.len() as u64;

        for r in &rows {
            let fresh = normalize_title(&r.title);
            if fresh != r.title_normalized {
                sqlx::query_file!(
                    "queries/admin/maintenance/playlist_set_title_norm.sql",
                    &r.urn,
                    fresh
                )
                .execute(pg)
                .await?;
                updated += 1;
            }
        }
    }
    info!(scanned, updated, "maintenance: playlist titles done");
    Ok(())
}

async fn unescape_track_meta(pg: &PgPool) -> AppResult<()> {
    let mut last = Uuid::nil();
    let mut fixed = 0u64;
    loop {
        let rows = sqlx::query_file!(
            "queries/admin/maintenance/tracks_meta_escaped_scan.sql",
            last,
            BATCH
        )
        .fetch_all(pg)
        .await?;
        let Some(tail) = rows.last() else { break };
        last = tail.id;

        for r in &rows {
            let Some(meta) = r.metadata_artist.as_deref() else {
                continue;
            };
            let fresh = unescape_json_unicode(meta);
            if fresh != meta {
                sqlx::query_file!("queries/admin/maintenance/track_set_meta.sql", r.id, fresh)
                    .execute(pg)
                    .await?;
                fixed += 1;
            }
        }
        info!(fixed, "maintenance: metadata_artist unescape progress");
    }
    info!(fixed, "maintenance: metadata_artist unescape done");
    Ok(())
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    e.as_database_error()
        .and_then(|d| d.code())
        .map(|c| c == "23505")
        .unwrap_or(false)
}
