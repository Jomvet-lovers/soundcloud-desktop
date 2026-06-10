//! Ко-лайк рёбра «фанаты тоже лайкают»: Ochiai-близость артистов по
//! пересечению лайкеров, с shrinkage против дутых малых выборок. Пересчёт
//! кроном (`cron.rs`); сетка волны читает их вместе с коллаб-рёбрами
//! `artist_coplay` (см. `graph/load_graph_edges.sql`).

use chrono::Utc;
use sqlx::PgPool;

use crate::error::AppResult;

/// Демпфер малых выборок: `co / (sqrt(la·lb) + S)` — пара с 2-3 общими
/// лайкерами при крошечных аудиториях не получает близость ~1.0.
const SHRINKAGE: f64 = 8.0;
/// Сколько рёбер на артиста держим (вхождение с любой из сторон).
const TOP_K_PER_ARTIST: i64 = 80;

/// Полный пересчёт: upsert свежих рёбер + выметание выпавших из топа.
pub async fn rebuild(pg: &PgPool) -> AppResult<u64> {
    let started = Utc::now();
    let ins = sqlx::query_file!(
        "queries/recommendations/smart_wave/colike/rebuild.sql",
        SHRINKAGE,
        TOP_K_PER_ARTIST
    )
    .execute(pg)
    .await?;
    sqlx::query_file!(
        "queries/recommendations/smart_wave/colike/prune.sql",
        started
    )
    .execute(pg)
    .await?;
    Ok(ins.rows_affected())
}

/// Поднять приоритет скачки/индексации pending-треков артистов из сеток
/// активных юзеров: каталог, который волна реально хочет отдавать, качается
/// раньше discovery-потока.
pub async fn bump_graph_storage_priority(pg: &PgPool) -> AppResult<u64> {
    let res =
        sqlx::query_file!("queries/recommendations/smart_wave/colike/bump_storage_priority.sql")
            .execute(pg)
            .await?;
    Ok(res.rows_affected())
}
