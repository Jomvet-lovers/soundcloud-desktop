//! Свежие сигналы юзера для SmartWave.
//!
//! Источник лайков — `user_likes_tracks` (зеркало `/me/likes/tracks`), читаем
//! `ORDER BY created_at DESC, ctid DESC`: ctid резолвит ties когда несколько
//! лайков пришли одним батчем рефреша с одинаковым `created_at` (видели на
//! проде для свежезалогиненных юзеров). Свежесть приоритетна — старые лайки
//! отрезает 365-дневное окно.
//!
//! Дизы, скипы, full_play идём в `user_events` — там события свежие по факту,
//! зеркало им не нужно.

use sqlx::PgPool;

use crate::error::AppResult;

/// Сколько свежих лайков рассматриваем как кандидатов для seed'а.
const FRESH_LIKES_LIMIT: i64 = 80;
/// Окно для дизов: всё что юзер дизлайкнул (хард-сигнал).
const DISLIKES_LIMIT: i64 = 200;
/// Скипы — мягкий негативный сигнал; берём только свежее.
const RECENT_SKIPS_DAYS: i32 = 30;
const RECENT_SKIPS_LIMIT: i64 = 60;
/// Full-play — мягкий позитив; берём свежее, чтобы не тянуть прошлогодний хвост.
const RECENT_PLAYED_DAYS: i32 = 30;
const RECENT_PLAYED_LIMIT: i64 = 200;
/// Полное окно "уже сыгранного" — для дедупа в волне, до года, без декея.
const PLAYED_DEDUPE_LIMIT: i64 = 500;

#[derive(Debug, Default)]
pub struct UserSignals {
    /// Свежие лайки из `user_likes_tracks`, отсортированные DESC по дате —
    /// первым идёт самый свежий, последний по списку — самый старый (но не
    /// старше 365 дней, иначе он бы сюда не попал).
    pub fresh_likes: Vec<String>,
    /// Жёсткие дизы — для qdrant negative + фильтра волны.
    pub disliked_ids: Vec<String>,
    /// Свежие скипы — мягкий негатив.
    pub recent_skips: Vec<String>,
    /// Сыгранное в последнее окно — для контекста "что сейчас слушает".
    pub recent_played: Vec<String>,
    /// Всё сыгранное за окно дедупа — чтобы волна не зацикливалась.
    pub played_dedupe: Vec<String>,
}

impl UserSignals {
    /// IDs, которые volna не должна повторять (дизы + всё сыгранное за окно).
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
    let likes_fut = load_fresh_likes(pg, sc_user_id);
    let dislikes_fut = load_dislikes(pg, sc_user_id);
    let skips_fut = load_recent_skips(pg, sc_user_id);
    let played_fut = load_recent_played(pg, sc_user_id);
    let dedupe_fut = load_played_dedupe(pg, sc_user_id);

    let (fresh_likes, disliked_ids, recent_skips, recent_played, played_dedupe) =
        tokio::try_join!(likes_fut, dislikes_fut, skips_fut, played_fut, dedupe_fut)?;

    Ok(UserSignals {
        fresh_likes,
        disliked_ids,
        recent_skips,
        recent_played,
        played_dedupe,
    })
}

async fn load_fresh_likes(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id \
         FROM user_likes_tracks \
         WHERE user_id = $1 AND wanted_state = true \
           AND created_at > NOW() - INTERVAL '365 days' \
         ORDER BY created_at DESC, ctid DESC \
         LIMIT $2",
    )
    .bind(sc_user_id)
    .bind(FRESH_LIKES_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn load_dislikes(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT sc_track_id FROM disliked_tracks WHERE sc_user_id = $1 LIMIT $2")
            .bind(sc_user_id)
            .bind(DISLIKES_LIMIT)
            .fetch_all(pg)
            .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn load_recent_skips(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT sc_track_id FROM user_events \
         WHERE sc_user_id = $1 AND event_type = 'skip' \
           AND created_at > NOW() - make_interval(days => $2::int) \
         ORDER BY created_at DESC LIMIT $3",
    )
    .bind(sc_user_id)
    .bind(RECENT_SKIPS_DAYS)
    .bind(RECENT_SKIPS_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn load_recent_played(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT ON (sc_track_id) sc_track_id, MAX(created_at) AS last_at \
         FROM user_events \
         WHERE sc_user_id = $1 AND event_type IN ('full_play', 'play_complete') \
           AND created_at > NOW() - make_interval(days => $2::int) \
         GROUP BY sc_track_id \
         ORDER BY sc_track_id, last_at DESC \
         LIMIT $3",
    )
    .bind(sc_user_id)
    .bind(RECENT_PLAYED_DAYS)
    .bind(RECENT_PLAYED_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

async fn load_played_dedupe(pg: &PgPool, sc_user_id: &str) -> AppResult<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT DISTINCT sc_track_id FROM user_events \
         WHERE sc_user_id = $1 \
           AND created_at > NOW() - INTERVAL '180 days' \
         ORDER BY sc_track_id \
         LIMIT $2",
    )
    .bind(sc_user_id)
    .bind(PLAYED_DEDUPE_LIMIT)
    .fetch_all(pg)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}
