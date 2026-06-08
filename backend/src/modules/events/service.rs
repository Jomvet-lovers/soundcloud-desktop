use std::sync::Arc;
use std::time::Duration;

use mini_moka::sync::Cache;
use sqlx::PgPool;
use tokio::sync::{Mutex as AsyncMutex, OnceCell};
use tracing::warn;

use crate::common::sc_ids::normalize_sc_track_id;
use crate::error::AppResult;
use crate::modules::collab::CollabTrainerService;
use crate::modules::dislikes::DislikesService;
use crate::modules::indexing::IndexingService;

const LIKE_WEIGHT: f64 = 1.0;
const PLAYLIST_ADD_WEIGHT: f64 = 0.9;
const FULL_PLAY_WEIGHT: f64 = 0.3;
const SKIP_WEIGHT: f64 = -0.5;
const DISLIKE_WEIGHT: f64 = -1.0;

const USER_LOCK_CAPACITY: u64 = 16_384;
const USER_LOCK_TTL: Duration = Duration::from_secs(5 * 60);

const POSITIVE_EVENTS: &[&str] = &["like", "playlist_add"];
const COLLAB_TRIGGER_EVENTS: &[&str] = &["like", "playlist_add", "full_play", "skip"];

fn event_weight(event_type: &str) -> Option<f64> {
    match event_type {
        "like" => Some(LIKE_WEIGHT),
        "playlist_add" => Some(PLAYLIST_ADD_WEIGHT),
        "full_play" => Some(FULL_PLAY_WEIGHT),
        "skip" => Some(SKIP_WEIGHT),
        "dislike" => Some(DISLIKE_WEIGHT),
        _ => None,
    }
}

async fn log_hard_negative_inline(
    pg: &PgPool,
    sc_user_id: &str,
    sc_track_id: &str,
    position_pct: f32,
) -> Result<(), sqlx::Error> {
    let predicted: Option<f32> = sqlx::query_scalar(
        "SELECT score FROM rec_impressions
         WHERE sc_user_id = ANY($1) AND sc_track_id = $2
         ORDER BY shown_at DESC LIMIT 1",
    )
        .bind(crate::common::sc_ids::user_id_variants(sc_user_id))
    .bind(sc_track_id)
    .fetch_optional(pg)
    .await?
    .flatten();
    let predicted = predicted.unwrap_or(0.0);
    sqlx::query(
        "INSERT INTO rec_hard_negatives (sc_user_id, sc_track_id, predicted_score, position_pct)
         VALUES ($1, $2, $3, $4)",
    )
    .bind(sc_user_id)
    .bind(sc_track_id)
    .bind(predicted)
    .bind(position_pct)
    .execute(pg)
    .await?;
    Ok(())
}

fn skip_weight_from_position(position_pct: Option<f32>) -> f64 {
    match position_pct {
        Some(p) if p < 0.20 => -0.8,
        Some(p) if p < 0.70 => -0.3,
        Some(_) => 0.0,
        None => SKIP_WEIGHT,
    }
}

pub struct EventsService {
    pg: PgPool,
    user_locks: Cache<String, Arc<AsyncMutex<()>>>,
    indexing: OnceCell<Arc<IndexingService>>,
    dislikes: OnceCell<Arc<DislikesService>>,
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
            indexing: OnceCell::new(),
            dislikes: OnceCell::new(),
            collab_trainer: OnceCell::new(),
        })
    }

    pub fn install_deps(
        &self,
        indexing: Arc<IndexingService>,
        dislikes: Arc<DislikesService>,
        collab_trainer: Arc<CollabTrainerService>,
    ) {
        let _ = self.indexing.set(indexing);
        let _ = self.dislikes.set(dislikes);
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

    fn enqueue_indexing(&self, sc_track_id: &str) {
        let Some(indexing) = self.indexing.get() else {
            return;
        };
        let svc = indexing.clone();
        let id = sc_track_id.to_string();
        tokio::spawn(async move {
            svc.trigger_indexing(&id).await;
        });
    }

    /// Записать событие юзера. Положительные события на disliked-трек тихо
    /// игнорятся — мы их не складываем в user_events. Любое новое событие
    /// триггерит indexing-очередь (idempotent), коллаб-инвалидацию и nudge
    /// тренера. SmartWave читает события напрямую из `user_events`, поэтому
    /// никаких "apply"/"taste" follow-up'ов больше нет.
    pub async fn record(
        self: &Arc<Self>,
        sc_user_id: &str,
        sc_track_id: &str,
        event_type: &str,
        position_pct: Option<f32>,
    ) -> AppResult<()> {
        let Some(mut weight) = event_weight(event_type) else {
            warn!(event_type, "Unknown event type");
            return Ok(());
        };
        let Some(normalized) = normalize_sc_track_id(sc_track_id) else {
            warn!(sc_track_id, "Invalid scTrackId");
            return Ok(());
        };

        let is_positive = POSITIVE_EVENTS.contains(&event_type);
        if is_positive {
            if let Some(d) = self.dislikes.get() {
                if d.is_disliked_by_user_id(sc_user_id, &normalized)
                    .await
                    .unwrap_or(false)
                {
                    // Лайк на трек в дизах — игнор, не загрязняем сигналы.
                    return Ok(());
                }
            }
        }

        if event_type == "skip" {
            weight = skip_weight_from_position(position_pct);
            if let Some(p) = position_pct {
                if p < 0.20 {
                    let pg = self.pg.clone();
                    let user = sc_user_id.to_string();
                    let id = normalized.clone();
                    tokio::spawn(async move {
                        if let Err(e) = log_hard_negative_inline(&pg, &user, &id, p).await {
                            warn!(error = %e, "hard_negative insert failed");
                        }
                    });
                }
            }
        }

        let lock_key = format!("events:{sc_user_id}");
        let lock = self.lock_for(&lock_key);
        let _g = lock.lock().await;

        sqlx::query(
            "INSERT INTO user_events (sc_user_id, sc_track_id, event_type, weight, position_pct) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(sc_user_id)
        .bind(&normalized)
        .bind(event_type)
        .bind(weight)
        .bind(position_pct)
        .execute(&self.pg)
        .await?;

        self.enqueue_indexing(&normalized);

        if COLLAB_TRIGGER_EVENTS.contains(&event_type) {
            if let Some(t) = self.collab_trainer.get() {
                t.note_event();
            }
        }
        Ok(())
    }
}
