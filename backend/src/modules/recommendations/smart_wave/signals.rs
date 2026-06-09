//! Свежие сигналы юзера для волны.
//!
//! Источник лайков — `user_likes_tracks` (зеркало `/me/likes/tracks`), читаем
//! `ORDER BY created_at DESC, ctid DESC`: ctid резолвит ties когда несколько
//! лайков пришли одним батчем рефреша с одинаковым `created_at`. Свежесть
//! приоритетна — старые лайки отрезает 365-дневное окно.
//!
//! Дизы, скипы, full_play идём в `user_events`. ВСЕ запросы матчим по обоим
//! формам `user_id` (URN + голый) — на проде сигналы расщеплены.

use sqlx::PgPool;

use crate::error::AppResult;
use crate::modules::recommendations::service::util::user_id_variants;

const FRESH_LIKES_LIMIT: i64 = 80;
const DISLIKES_LIMIT: i64 = 200;
const RECENT_SKIPS_DAYS: i32 = 30;
const RECENT_SKIPS_LIMIT: i64 = 60;
const RECENT_PLAYED_DAYS: i32 = 30;
const RECENT_PLAYED_LIMIT: i64 = 200;

#[derive(Debug, Default)]
pub struct UserSignals {
    /// Свежие лайки (DESC по дате; первый — самый свежий).
    pub fresh_likes: Vec<String>,
    /// Жёсткие дизы — для qdrant negative + фильтра волны.
    pub disliked_ids: Vec<String>,
    /// Свежие скипы — мягкий негатив.
    pub recent_skips: Vec<String>,
    /// Сыгранное в последнее окно — контекст «что сейчас слушает».
    pub recent_played: Vec<String>,
}

impl UserSignals {
    /// Жёсткое исключение ВСЕГДА — только дизы. «Прослушанное» добавляется
    /// отдельно в build по тогглу `hide_listened` (тиерно 7/14/30д); скипы
    /// остаются мягким негативом для qdrant, но из выдачи не режутся.
    pub fn always_exclude(&self) -> Vec<String> {
        let mut v = self.disliked_ids.clone();
        v.sort();
        v.dedup();
        v
    }
}

pub async fn load_recent_signals(pg: &PgPool, sc_user_id: &str) -> AppResult<UserSignals> {
    let ids = user_id_variants(sc_user_id);

    let (fresh_likes, disliked_ids, recent_skips, recent_played) = tokio::try_join!(
        load_fresh_likes(pg, &ids),
        load_dislikes(pg, &ids),
        load_recent_skips(pg, &ids),
        load_recent_played(pg, &ids),
    )?;

    Ok(UserSignals {
        fresh_likes,
        disliked_ids,
        recent_skips,
        recent_played,
    })
}

/// «Скрыть прослушанное» — тиерный сет id (лайк 7д · full_play 14д · skip 30д
/// от последнего прослуша). Пусто = тоггл выключен или нечего скрывать.
pub(crate) async fn load_hidden_by_listen(pg: &PgPool, ids: &[String]) -> Vec<String> {
    sqlx::query_file_scalar!(
        "queries/recommendations/smart_wave/signals/hidden_by_listen.sql",
        ids
    )
    .fetch_all(pg)
    .await
    .unwrap_or_default()
}

async fn load_fresh_likes(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows = sqlx::query_file_scalar!(
        "queries/recommendations/smart_wave/signals/fresh_likes.sql",
        ids,
        FRESH_LIKES_LIMIT
    )
    .fetch_all(pg)
    .await?;
    Ok(dedup_keep_order(rows))
}

async fn load_dislikes(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows = sqlx::query_file_scalar!(
        "queries/recommendations/smart_wave/signals/dislikes.sql",
        ids,
        DISLIKES_LIMIT
    )
    .fetch_all(pg)
    .await?;
    Ok(dedup_keep_order(rows))
}

async fn load_recent_skips(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows = sqlx::query_file_scalar!(
        "queries/recommendations/smart_wave/signals/recent_skips.sql",
        ids,
        RECENT_SKIPS_DAYS,
        RECENT_SKIPS_LIMIT
    )
    .fetch_all(pg)
    .await?;
    Ok(dedup_keep_order(rows))
}

async fn load_recent_played(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows = sqlx::query_file_scalar!(
        "queries/recommendations/smart_wave/signals/recent_played.sql",
        ids,
        RECENT_PLAYED_DAYS,
        RECENT_PLAYED_LIMIT
    )
    .fetch_all(pg)
    .await?;
    Ok(rows)
}

fn dedup_keep_order(rows: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .filter(|id| seen.insert(id.clone()))
        .collect()
}
