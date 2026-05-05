use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use chrono::NaiveDateTime;
use mini_moka::sync::Cache;
use sqlx::{FromRow, PgPool};
use tokio::sync::{Mutex as AsyncMutex, OnceCell};
use tracing::{error, warn};
use uuid::Uuid;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;
use crate::modules::collab::{CollabTrainerService, CollabVectorService};
use crate::modules::dislikes::DislikesService;
use crate::modules::indexing::IndexingService;
use crate::modules::user_taste::UserTasteService;

const LIKE_WEIGHT: f64 = 1.0;
const LOCAL_LIKE_WEIGHT: f64 = 1.0;
const PLAYLIST_ADD_WEIGHT: f64 = 0.9;
const FULL_PLAY_WEIGHT: f64 = 0.3;
const SKIP_WEIGHT: f64 = -0.5;
const DISLIKE_WEIGHT: f64 = -1.0;

const USER_LOCK_CAPACITY: u64 = 16_384;
const USER_LOCK_TTL: Duration = Duration::from_secs(5 * 60);

const POSITIVE_EVENTS: &[&str] = &["like", "local_like", "playlist_add"];
const COLLAB_TRIGGER_EVENTS: &[&str] = &["like", "local_like", "playlist_add", "full_play", "skip"];

fn event_weight(event_type: &str) -> Option<f64> {
    match event_type {
        "like" => Some(LIKE_WEIGHT),
        "local_like" => Some(LOCAL_LIKE_WEIGHT),
        "playlist_add" => Some(PLAYLIST_ADD_WEIGHT),
        "full_play" => Some(FULL_PLAY_WEIGHT),
        "skip" => Some(SKIP_WEIGHT),
        "dislike" => Some(DISLIKE_WEIGHT),
        _ => None,
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct UserEventRow {
    pub id: Uuid,
    pub sc_user_id: String,
    pub sc_track_id: String,
    pub event_type: String,
    pub weight: f64,
    pub seeded: bool,
    pub created_at: NaiveDateTime,
    pub taste_applied_at: Option<NaiveDateTime>,
}

pub struct EventsService {
    pg: PgPool,
    user_locks: Cache<String, Arc<AsyncMutex<()>>>,
    user_taste: OnceCell<Arc<UserTasteService>>,
    indexing: OnceCell<Arc<IndexingService>>,
    dislikes: OnceCell<Arc<DislikesService>>,
    collab: OnceCell<Arc<CollabVectorService>>,
    collab_trainer: OnceCell<Arc<CollabTrainerService>>,
}

impl EventsService {
    pub fn new(pg: PgPool) -> Arc<Self> {
        Arc::new(Self {
            pg,
            user_locks: Cache::builder()
                .max_capacity(USER_LOCK_CAPACITY)
                .time_to_idle(USER_LOCK_TTL)
                .build(),
            user_taste: OnceCell::new(),
            indexing: OnceCell::new(),
            dislikes: OnceCell::new(),
            collab: OnceCell::new(),
            collab_trainer: OnceCell::new(),
        })
    }

    pub fn install_deps(
        &self,
        user_taste: Arc<UserTasteService>,
        indexing: Arc<IndexingService>,
        dislikes: Arc<DislikesService>,
        collab: Arc<CollabVectorService>,
        collab_trainer: Arc<CollabTrainerService>,
    ) {
        let _ = self.user_taste.set(user_taste);
        let _ = self.indexing.set(indexing);
        let _ = self.dislikes.set(dislikes);
        let _ = self.collab.set(collab);
        let _ = self.collab_trainer.set(collab_trainer);
    }

    fn lock_for(&self, key: &str) -> Arc<AsyncMutex<()>> {
        if let Some(lock) = self.user_locks.get(&key.to_string()) {
            return lock;
        }
        let lock = Arc::new(AsyncMutex::new(()));
        self.user_locks.insert(key.to_string(), lock.clone());
        lock
    }

    async fn mark_applied(&self, id: Uuid) -> AppResult<()> {
        sqlx::query("UPDATE user_events SET taste_applied_at = now() WHERE id = $1")
            .bind(id)
            .execute(&self.pg)
            .await?;
        Ok(())
    }

    async fn try_apply(&self, event: &UserEventRow) -> AppResult<bool> {
        let dislikes = self.dislikes.get();
        let user_taste = self.user_taste.get();
        let is_positive = POSITIVE_EVENTS.contains(&event.event_type.as_str());

        let is_disliked = match dislikes {
            Some(d) => d
                .is_disliked_by_user_id(&event.sc_user_id, &event.sc_track_id)
                .await
                .unwrap_or(false),
            None => false,
        };

        if is_positive && is_disliked {
            self.mark_applied(event.id).await?;
            return Ok(true);
        }
        if !is_positive {
            self.mark_applied(event.id).await?;
            return Ok(true);
        }

        let Some(user_taste) = user_taste else {
            return Ok(false);
        };
        let applied = user_taste
            .on_user_event(&event.sc_user_id, &event.sc_track_id, &event.event_type)
            .await?;
        if applied {
            self.mark_applied(event.id).await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn record(
        self: &Arc<Self>,
        sc_user_id: &str,
        sc_track_id: &str,
        event_type: &str,
    ) -> AppResult<()> {
        let Some(weight) = event_weight(event_type) else {
            warn!(event_type, "Unknown event type");
            return Ok(());
        };
        let Some(normalized) = normalize_sc_track_id(sc_track_id) else {
            warn!(sc_track_id, "Invalid scTrackId");
            return Ok(());
        };

        let lock_key = format!("events:{sc_user_id}");
        let lock = self.lock_for(&lock_key);
        let _g = lock.lock().await;

        let event: UserEventRow = sqlx::query_as(
            "INSERT INTO user_events (sc_user_id, sc_track_id, event_type, weight, seeded) \
             VALUES ($1, $2, $3, $4, false) RETURNING id, sc_user_id, sc_track_id, event_type, weight, seeded, created_at, taste_applied_at",
        )
        .bind(sc_user_id)
        .bind(&normalized)
        .bind(event_type)
        .bind(weight)
        .fetch_one(&self.pg)
        .await?;

        let applied = self.try_apply(&event).await?;
        if !applied {
            if let Some(indexing) = self.indexing.get() {
                let svc = indexing.clone();
                let id = normalized.clone();
                tokio::spawn(async move {
                    if let Err(e) = svc.ensure_track_queued_by_id(&id).await {
                        error!(track = %id, error = %e, "Failed to enqueue");
                    }
                });
            }
        }
        if POSITIVE_EVENTS.contains(&event_type) {
            if let Some(c) = self.collab.get() {
                c.invalidate(sc_user_id);
            }
        }
        if COLLAB_TRIGGER_EVENTS.contains(&event_type) {
            if let Some(t) = self.collab_trainer.get() {
                t.note_event();
            }
        }
        Ok(())
    }

    pub async fn ensure_likes_recorded(
        self: &Arc<Self>,
        sc_user_id: &str,
        sc_track_ids: &[String],
    ) -> AppResult<()> {
        if sc_user_id.is_empty() || sc_track_ids.is_empty() {
            return Ok(());
        }
        let normalized_all: Vec<String> = sc_track_ids
            .iter()
            .filter_map(|s| normalize_sc_track_id(s))
            .collect();
        if normalized_all.is_empty() {
            return Ok(());
        }

        let lock_key = format!("events:{sc_user_id}");
        let lock = self.lock_for(&lock_key);
        let _g = lock.lock().await;

        let existing: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events \
             WHERE sc_user_id = $1 AND event_type = 'like' AND sc_track_id = ANY($2)",
        )
        .bind(sc_user_id)
        .bind(&normalized_all)
        .fetch_all(&self.pg)
        .await?;
        let existing_set: HashSet<String> = existing.into_iter().map(|(s,)| s).collect();

        let mut missing: Vec<String> = normalized_all
            .into_iter()
            .filter(|id| !existing_set.contains(id))
            .collect();
        missing.sort();
        missing.dedup();
        if missing.is_empty() {
            return Ok(());
        }

        let n = missing.len();
        let user_ids = vec![sc_user_id.to_string(); n];
        let types = vec!["like".to_string(); n];
        let weights = vec![LIKE_WEIGHT; n];
        let seeded = vec![true; n];

        let saved: Vec<UserEventRow> = sqlx::query_as(
            "INSERT INTO user_events (sc_user_id, sc_track_id, event_type, weight, seeded) \
             SELECT * FROM UNNEST($1::text[], $2::text[], $3::text[], $4::float8[], $5::bool[]) \
             RETURNING id, sc_user_id, sc_track_id, event_type, weight, seeded, created_at, taste_applied_at",
        )
        .bind(&user_ids)
        .bind(&missing)
        .bind(&types)
        .bind(&weights)
        .bind(&seeded)
        .fetch_all(&self.pg)
        .await?;

        for event in saved {
            let applied = self.try_apply(&event).await.unwrap_or(false);
            if !applied {
                if let Some(indexing) = self.indexing.get() {
                    let svc = indexing.clone();
                    let id = event.sc_track_id.clone();
                    tokio::spawn(async move {
                        if let Err(e) = svc.ensure_track_queued_by_id(&id).await {
                            error!(track = %id, error = %e, "Failed to enqueue");
                        }
                    });
                }
            }
        }
        Ok(())
    }

    pub async fn apply_pending_events_for_track(self: &Arc<Self>, sc_track_id: &str) -> AppResult<()> {
        let pending: Vec<UserEventRow> = sqlx::query_as(
            "SELECT id, sc_user_id, sc_track_id, event_type, weight, seeded, created_at, taste_applied_at \
             FROM user_events WHERE sc_track_id = $1 AND taste_applied_at IS NULL \
             ORDER BY created_at ASC",
        )
        .bind(sc_track_id)
        .fetch_all(&self.pg)
        .await?;
        if pending.is_empty() {
            return Ok(());
        }

        let mut by_user: HashMap<String, Vec<UserEventRow>> = HashMap::new();
        for e in pending {
            by_user.entry(e.sc_user_id.clone()).or_default().push(e);
        }

        let mut tasks = Vec::new();
        for (user_id, events) in by_user {
            let svc = self.clone();
            tasks.push(tokio::spawn(async move {
                let lock_key = format!("events:{user_id}");
                let lock = svc.lock_for(&lock_key);
                let _g = lock.lock().await;
                for event in events {
                    if let Err(e) = svc.try_apply(&event).await {
                        error!(event_id = %event.id, error = %e, "tryApply failed");
                    }
                }
            }));
        }
        for t in tasks {
            let _ = t.await;
        }
        Ok(())
    }

    pub async fn get_recent_liked(&self, sc_user_id: &str, limit: i64) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events \
             WHERE sc_user_id = $1 AND event_type = 'like' \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(sc_user_id)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    pub async fn get_recent_skipped(&self, sc_user_id: &str, limit: i64) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events \
             WHERE sc_user_id = $1 AND event_type = 'skip' \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(sc_user_id)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows.into_iter().map(|(s,)| s).collect())
    }

    pub async fn get_recent_played(&self, sc_user_id: &str, limit: i64) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events \
             WHERE sc_user_id = $1 \
             ORDER BY created_at DESC LIMIT $2",
        )
        .bind(sc_user_id)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;
        let mut seen: HashSet<String> = HashSet::new();
        let mut out: Vec<String> = Vec::new();
        for (id,) in rows {
            if seen.insert(id.clone()) {
                out.push(id);
            }
        }
        Ok(out)
    }

    pub fn spawn_indexing_queue_consumer(self: &Arc<Self>, nats: Arc<crate::bus::nats::NatsService>) {
        let svc = self.clone();
        nats.consume(
            crate::bus::subjects::streams::DONE.name,
            "backend-events-done",
            None,
            move |data, _meta| {
                let svc = svc.clone();
                async move {
                    let sc_track_id = match data.get("sc_track_id") {
                        Some(v) if v.is_string() => v.as_str().unwrap_or("").to_string(),
                        Some(v) if v.is_number() => v.to_string(),
                        _ => String::new(),
                    };
                    if sc_track_id.is_empty() {
                        return Ok(());
                    }
                    if let Err(e) = svc.apply_pending_events_for_track(&sc_track_id).await {
                        error!(track = %sc_track_id, error = %e, "apply pending failed");
                    }
                    Ok(())
                }
            },
        );
    }
}
