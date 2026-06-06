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
const PLAYED_DEDUPE_LIMIT: i64 = 500;

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
    /// Всё сыгранное за окно дедупа — чтобы волна не зацикливалась.
    pub played_dedupe: Vec<String>,
}

impl UserSignals {
    /// IDs, которые волна не должна повторять (дизы + сыгранное + скипы).
    pub fn exclude_set(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::with_capacity(
            self.disliked_ids.len() + self.played_dedupe.len() + self.recent_skips.len(),
        );
        v.extend(self.disliked_ids.iter().cloned());
        v.extend(self.played_dedupe.iter().cloned());
        v.extend(self.recent_skips.iter().cloned());
        v.sort();
        v.dedup();
        v
    }
}

pub async fn load_recent_signals(pg: &PgPool, sc_user_id: &str) -> AppResult<UserSignals> {
    let ids = user_id_variants(sc_user_id);

    let (fresh_likes, disliked_ids, recent_skips, recent_played, played_dedupe) = tokio::try_join!(
        load_fresh_likes(pg, &ids),
        load_dislikes(pg, &ids),
        load_recent_skips(pg, &ids),
        load_recent_played(pg, &ids),
        load_played_dedupe(pg, &ids),
    )?;

    Ok(UserSignals {
        fresh_likes,
        disliked_ids,
        recent_skips,
        recent_played,
        played_dedupe,
    })
}

async fn load_fresh_likes(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id \
         FROM user_likes_tracks \
         WHERE user_id = ANY($1) AND wanted_state = true \
           AND created_at > NOW() - INTERVAL '365 days' \
         ORDER BY created_at DESC, ctid DESC \
         LIMIT $2",
    )
        .bind(ids)
    .bind(FRESH_LIKES_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(dedup_keep_order(rows))
}

async fn load_dislikes(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id FROM disliked_tracks WHERE sc_user_id = ANY($1) LIMIT $2",
    )
        .bind(ids)
        .bind(DISLIKES_LIMIT)
        .fetch_all(pg)
        .await?;
    Ok(dedup_keep_order(rows))
}

async fn load_recent_skips(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id FROM user_events \
         WHERE sc_user_id = ANY($1) AND event_type = 'skip' \
           AND created_at > NOW() - make_interval(days => $2::int) \
         ORDER BY created_at DESC LIMIT $3",
    )
        .bind(ids)
    .bind(RECENT_SKIPS_DAYS)
    .bind(RECENT_SKIPS_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(dedup_keep_order(rows))
}

async fn load_recent_played(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id \
         FROM user_events \
         WHERE sc_user_id = ANY($1) AND event_type IN ('full_play', 'play_complete') \
           AND created_at > NOW() - make_interval(days => $2::int) \
         GROUP BY sc_track_id \
         ORDER BY MAX(created_at) DESC \
         LIMIT $3",
    )
        .bind(ids)
    .bind(RECENT_PLAYED_DAYS)
    .bind(RECENT_PLAYED_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn load_played_dedupe(pg: &PgPool, ids: &[String]) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT sc_track_id FROM user_events \
         WHERE sc_user_id = ANY($1) \
           AND created_at > NOW() - INTERVAL '180 days' \
         ORDER BY sc_track_id \
         LIMIT $2",
    )
        .bind(ids)
    .bind(PLAYED_DEDUPE_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

fn dedup_keep_order(rows: Vec<(String,)>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    rows.into_iter()
        .map(|(id, )| id)
        .filter(|id| seen.insert(id.clone()))
        .collect()
}
