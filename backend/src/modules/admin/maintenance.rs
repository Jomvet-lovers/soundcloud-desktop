//! Разовые фоновые чистки каталога после смены алгоритмов имён:
//!
//!   * перенормализация `artists.normalized_name` под новый fold
//!     (ᴍᴏɴᴀʀᴄʜ → monarch); коллизия ключа = тот же артист, записанный
//!     по-разному — помечаем `merged_into` на владельца ключа;
//!   * расшивка литеральных `\uXXXX` в `tracks.metadata_artist`.
//!
//! POST /admin/maintenance/renormalize — идемпотентно, повторный вызов на
//! уже чистых данных ничего не меняет. Работает батчами в фоне, прогресс в логе.

use std::sync::atomic::{AtomicBool, Ordering};

use axum::extract::State;
use axum::Json;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{info, warn};
use uuid::Uuid;

use crate::common::admin::AdminAuth;
use crate::error::AppResult;
use crate::modules::enrich::artist_names::unescape_json_unicode;
use crate::modules::enrich::normalize::normalize_name;
use crate::state::AppState;

static RUNNING: AtomicBool = AtomicBool::new(false);

const BATCH: i64 = 5_000;

#[tracing::instrument(skip_all)]
pub async fn renormalize(_: AdminAuth, State(st): State<AppState>) -> AppResult<Json<Value>> {
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(Json(
            json!({ "started": false, "reason": "already running" }),
        ));
    }
    let pg = st.pg.clone();
    tokio::spawn(async move {
        let res = run(&pg).await;
        RUNNING.store(false, Ordering::SeqCst);
        if let Err(e) = res {
            warn!(error = %e, "maintenance renormalize failed");
        }
    });
    Ok(Json(json!({ "started": true })))
}

async fn run(pg: &PgPool) -> AppResult<()> {
    renormalize_artists(pg).await?;
    unescape_track_meta(pg).await?;
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
                    // Помечаем merged_into — новые апсерты пойдут во владельца.
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
