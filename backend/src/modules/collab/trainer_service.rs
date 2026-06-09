use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use futures::TryStreamExt;
use serde::Deserialize;
use serde_json::json;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::bus::nats::NatsService;
use crate::bus::subjects::{self, streams};
use crate::config::CollabCfg;
use crate::error::AppResult;
use crate::modules::collab::vector_service::CollabVectorService;
use crate::qdrant::QdrantService;

#[derive(Debug, Deserialize)]
struct CollabBlob {
    dim: u64,
    points: Vec<CollabPoint>,
}

#[derive(Debug, Deserialize)]
struct CollabPoint {
    id: u64,
    vec: Vec<f32>,
}

const SESSION_GAP_MS: i64 = 30 * 60 * 1000;
const MIN_SESSION_LEN: usize = 2;
const MAX_SESSION_LEN: usize = 200;
const HISTORY_WINDOW_DAYS: i64 = 90;
const SESSION_EVENTS: &[&str] = &["like", "playlist_add", "full_play", "skip"];

pub struct CollabTrainerService {
    pg: PgPool,
    nats: Arc<NatsService>,
    qdrant: Arc<QdrantService>,
    collab: Arc<CollabVectorService>,
    cfg: CollabCfg,
    in_progress: AtomicBool,
    event_counter: AtomicU64,
    last_train_at_ms: AtomicI64,
}

#[derive(Debug, Clone)]
pub struct TrainResult {
    pub enqueued: bool,
    pub sessions: usize,
    pub reason: Option<String>,
}

impl CollabTrainerService {
    pub fn new(
        pg: PgPool,
        nats: Arc<NatsService>,
        qdrant: Arc<QdrantService>,
        collab: Arc<CollabVectorService>,
        cfg: CollabCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            nats,
            qdrant,
            collab,
            cfg,
            in_progress: AtomicBool::new(false),
            event_counter: AtomicU64::new(0),
            last_train_at_ms: AtomicI64::new(0),
        })
    }

    /// Воркер тренирует Word2Vec и кладёт вектора блобом в Object Store, в
    /// `done.train_collab` едет имя объекта + dim. Backend читает блоб и пишет
    /// в Qdrant — единственная точка записи collab-векторов.
    fn spawn_done_consumer(self: &Arc<Self>) {
        let svc = self.clone();
        self.nats.consume(
            streams::DONE.name,
            "backend-done-train-collab",
            Some(subjects::DONE_TRAIN_COLLAB),
            1,
            move |data| {
                let svc = svc.clone();
                async move {
                    if !data
                        .get("trained")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false)
                    {
                        return Ok(());
                    }
                    // Нет object → старый воркер писал в Qdrant сам (rolling deploy).
                    let Some(object) = data.get("object").and_then(|v| v.as_str()) else {
                        return Ok(());
                    };
                    let blob: CollabBlob = svc
                        .nats
                        .get_object(subjects::COLLAB_DATA_BUCKET, object)
                        .await?;
                    if blob.dim == 0 {
                        warn!(object, "[collab] blob dim=0, skip");
                        let _ = svc
                            .nats
                            .delete_object(subjects::COLLAB_DATA_BUCKET, object)
                            .await;
                        return Ok(());
                    }
                    let points: Vec<(u64, Vec<f32>)> =
                        blob.points.into_iter().map(|p| (p.id, p.vec)).collect();
                    let n = svc.qdrant.upsert_collab(blob.dim, points).await?;
                    if let Err(e) = svc
                        .nats
                        .delete_object(subjects::COLLAB_DATA_BUCKET, object)
                        .await
                    {
                        warn!(object, error = %e, "[collab] vectors object cleanup failed");
                    }
                    svc.collab.invalidate_all();
                    info!(
                        count = n,
                        dim = blob.dim,
                        "[collab] vectors upserted from worker"
                    );
                    Ok(())
                }
            },
        );
    }

    pub fn spawn_bootstrap_and_cron(self: &Arc<Self>, shutdown: CancellationToken) {
        self.spawn_done_consumer();
        let svc = self.clone();
        let token = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(30)) => {}
            }
            svc.bootstrap_if_needed().await;
        });

        if self.cfg.auto_train {
            let svc = self.clone();
            let token = shutdown.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(6 * 60 * 60));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await;
                loop {
                    tokio::select! {
                        _ = token.cancelled() => break,
                        _ = ticker.tick() => {
                            if let Err(e) = svc.train_now(None, None).await {
                                warn!(error = %e, "scheduled collab train failed");
                            }
                        }
                    }
                }
            });
        }
    }

    async fn bootstrap_if_needed(self: &Arc<Self>) {
        let dim = self.collab.get_collab_dim().await;
        if dim.is_some() {
            info!(dim = ?dim, "[collab.bootstrap] tracks_collab exists, skip initial train");
            return;
        }
        info!("[collab.bootstrap] tracks_collab missing, triggering initial train");
        match self.train_now(None, None).await {
            Ok(res) => {
                info!(enqueued = res.enqueued, sessions = res.sessions, reason = ?res.reason, "[collab.bootstrap] result")
            }
            Err(e) => warn!(error = %e, "[collab.bootstrap] failed"),
        }
    }

    pub fn note_event(self: &Arc<Self>) {
        let count = self.event_counter.fetch_add(1, Ordering::Relaxed) + 1;
        if (count as u32) < self.cfg.trigger_events {
            return;
        }
        let now_ms = Utc::now().timestamp_millis();
        let last = self.last_train_at_ms.load(Ordering::Relaxed);
        if now_ms - last < self.cfg.trigger_cooldown_ms as i64 {
            return;
        }
        if self.in_progress.load(Ordering::Relaxed) {
            return;
        }
        self.event_counter.store(0, Ordering::Relaxed);
        info!("[collab.auto] threshold reached, triggering train");
        let svc = self.clone();
        tokio::spawn(async move {
            if let Err(e) = svc.train_now(None, None).await {
                warn!(error = %e, "auto train failed");
            }
        });
    }

    pub async fn train_now(
        self: &Arc<Self>,
        dim_override: Option<u32>,
        min_count_override: Option<u32>,
    ) -> AppResult<TrainResult> {
        if self
            .in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(TrainResult {
                enqueued: false,
                sessions: 0,
                reason: Some("already_in_progress".into()),
            });
        }
        let result = self.train_impl(dim_override, min_count_override).await;
        self.in_progress.store(false, Ordering::Release);
        result
    }

    async fn train_impl(
        self: &Arc<Self>,
        dim_override: Option<u32>,
        min_count_override: Option<u32>,
    ) -> AppResult<TrainResult> {
        let sessions = self.build_sessions().await?;
        let min_sessions = self.cfg.min_sessions as usize;
        if sessions.len() < min_sessions {
            warn!(
                count = sessions.len(),
                min = min_sessions,
                "[collab.train] too few sessions, skip"
            );
            return Ok(TrainResult {
                enqueued: false,
                sessions: sessions.len(),
                reason: Some("too_few_sessions".into()),
            });
        }
        let dim = dim_override.unwrap_or(self.cfg.dim);
        let min_count = min_count_override.unwrap_or(self.cfg.min_count);
        info!(
            sessions = sessions.len(),
            dim, min_count, "[collab.train] enqueuing"
        );
        let object = format!("collab-{}", Utc::now().timestamp_millis());
        self.nats
            .put_object(subjects::COLLAB_DATA_BUCKET, &object, &sessions)
            .await?;
        let payload = json!({
            "object": object,
            "dim": dim,
            "min_count": min_count,
            "window": 5,
            "epochs": 5,
            "negative": 10,
        });
        self.nats.publish(subjects::TRAIN_COLLAB, &payload).await?;
        self.collab.invalidate_all();
        self.last_train_at_ms
            .store(Utc::now().timestamp_millis(), Ordering::Relaxed);
        Ok(TrainResult {
            enqueued: true,
            sessions: sessions.len(),
            reason: None,
        })
    }

    async fn build_sessions(&self) -> AppResult<Vec<Vec<u64>>> {
        let since = Utc::now().naive_utc() - chrono::Duration::days(HISTORY_WINDOW_DAYS);
        let session_events: Vec<String> = SESSION_EVENTS.iter().map(|s| s.to_string()).collect();
        // Стримим, а не fetch_all: весь 90д-набор не материализуем в RAM и не
        // пиним коннект под огромный буфер. event_type фильтруем в SQL.
        let mut stream = sqlx::query_file!(
            "queries/collab/trainer_service/build_sessions.sql",
            since,
            &session_events
        )
        .fetch(&self.pg);
        let mut total_rows = 0usize;

        let mut sessions: Vec<Vec<u64>> = Vec::new();
        let mut current_user: Option<String> = None;
        let mut current_time_ms: i64 = 0;
        let mut current_session: Vec<u64> = Vec::new();
        let mut current_seen: std::collections::HashSet<u64> = std::collections::HashSet::new();

        let session_set: std::collections::HashSet<&str> = SESSION_EVENTS.iter().copied().collect();

        let flush = |session: &mut Vec<u64>,
                     seen: &mut std::collections::HashSet<u64>,
                     out: &mut Vec<Vec<u64>>| {
            if session.len() >= MIN_SESSION_LEN {
                let mut copy = session.clone();
                if copy.len() > MAX_SESSION_LEN {
                    copy.truncate(MAX_SESSION_LEN);
                }
                out.push(copy);
            }
            session.clear();
            seen.clear();
        };

        while let Some(row) = stream.try_next().await? {
            let (sc_user_id, sc_track_id, created_at, event_type) = (
                row.sc_user_id,
                row.sc_track_id,
                row.created_at,
                row.event_type,
            );
            total_rows += 1;
            if !session_set.contains(event_type.as_str()) {
                continue;
            }
            let Ok(tid) = sc_track_id.parse::<u64>() else {
                continue;
            };
            let ts_ms = created_at.and_utc().timestamp_millis();

            match &current_user {
                None => {
                    current_user = Some(sc_user_id.clone());
                    current_time_ms = ts_ms;
                }
                Some(u) if *u != sc_user_id => {
                    flush(&mut current_session, &mut current_seen, &mut sessions);
                    current_user = Some(sc_user_id.clone());
                    current_time_ms = ts_ms;
                }
                Some(_) => {
                    if ts_ms - current_time_ms > SESSION_GAP_MS {
                        flush(&mut current_session, &mut current_seen, &mut sessions);
                    }
                    current_time_ms = ts_ms;
                }
            }

            if current_seen.insert(tid) {
                current_session.push(tid);
            }
        }
        flush(&mut current_session, &mut current_seen, &mut sessions);

        info!(
            sessions = sessions.len(),
            events = total_rows,
            window_days = HISTORY_WINDOW_DAYS,
            "[collab.train] built sessions"
        );
        Ok(sessions)
    }
}
