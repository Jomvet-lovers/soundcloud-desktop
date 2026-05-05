use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use qdrant_client::qdrant::{
    point_id::PointIdOptions, vectors_output::VectorsOptions, GetPointsBuilder, PointId,
    VectorOutput,
};
use serde_json::Value;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::common::user_id::user_id_to_qdrant_id;
use crate::config::LtrCfg;
use crate::error::AppResult;
use crate::modules::centroids::{cosine, CentroidService};
use crate::modules::collab::CollabVectorService;
use crate::modules::ltr::service::{LtrExample, LtrService, LTR_FEATURE_COUNT};
use crate::qdrant::{collections, QdrantService};

const TRAIN_WINDOW_DAYS: i64 = 30;
const MIN_POSITIVE_PER_USER: i64 = 10;
const MAX_USERS: i64 = 500;
const MAX_PAIRS_PER_USER: usize = 80;
const MIN_NEGATIVES_PER_USER: i64 = 8;
const MIN_TOTAL_EXAMPLES: usize = 500;

const POSITIVE_TYPES: &[&str] = &["like", "local_like", "playlist_add"];
const ALL_TYPES: &[&str] = &["like", "local_like", "playlist_add", "full_play", "skip"];

fn label_for(event_type: &str) -> Option<i32> {
    match event_type {
        "like" | "local_like" => Some(5),
        "playlist_add" => Some(4),
        "full_play" => Some(3),
        "skip" => Some(0),
        _ => None,
    }
}

pub struct LtrTrainerService {
    pg: PgPool,
    qdrant: Arc<QdrantService>,
    collab: Arc<CollabVectorService>,
    centroids: Arc<CentroidService>,
    ltr: Arc<LtrService>,
    cfg: LtrCfg,
    in_progress: AtomicBool,
}

#[derive(Debug, Clone)]
pub struct TrainResult {
    pub enqueued: bool,
    pub examples: usize,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct TrackVectors {
    collab: Option<Vec<f32>>,
    mert: Option<Vec<f32>>,
    clap: Option<Vec<f32>>,
    lyrics: Option<Vec<f32>>,
    playback_count: i64,
    language: Option<String>,
}

impl LtrTrainerService {
    pub fn new(
        pg: PgPool,
        qdrant: Arc<QdrantService>,
        collab: Arc<CollabVectorService>,
        centroids: Arc<CentroidService>,
        ltr: Arc<LtrService>,
        cfg: LtrCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            pg,
            qdrant,
            collab,
            centroids,
            ltr,
            cfg,
            in_progress: AtomicBool::new(false),
        })
    }

    pub fn spawn_bootstrap_and_cron(self: &Arc<Self>, shutdown: CancellationToken) {
        let svc = self.clone();
        let token = shutdown.clone();
        tokio::spawn(async move {
            tokio::select! {
                _ = token.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(5 * 60)) => {}
            }
            if let Err(e) = svc.train_now().await {
                debug!(error = %e, "bootstrap ltr-train failed");
            }
        });

        if self.cfg.auto_train {
            let svc = self.clone();
            let token = shutdown.clone();
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(Duration::from_secs(60 * 60));
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                ticker.tick().await;
                loop {
                    tokio::select! {
                        _ = token.cancelled() => break,
                        _ = ticker.tick() => {
                            if should_run_weekly() {
                                if let Err(e) = svc.train_now().await {
                                    warn!(error = %e, "scheduled ltr-train failed");
                                }
                            }
                        }
                    }
                }
            });
        }
    }

    pub async fn train_now(self: &Arc<Self>) -> AppResult<TrainResult> {
        if self
            .in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(TrainResult {
                enqueued: false,
                examples: 0,
                reason: Some("in_progress".into()),
            });
        }
        let result = self.train_impl().await;
        self.in_progress.store(false, Ordering::Release);
        result
    }

    async fn train_impl(self: &Arc<Self>) -> AppResult<TrainResult> {
        let examples = self.build_examples().await?;
        if examples.len() < MIN_TOTAL_EXAMPLES {
            warn!(
                count = examples.len(),
                min = MIN_TOTAL_EXAMPLES,
                "[ltr.train] too few examples, skip"
            );
            return Ok(TrainResult {
                enqueued: false,
                examples: examples.len(),
                reason: Some("too_few_examples".into()),
            });
        }
        info!(count = examples.len(), "[ltr.train] publishing");
        self.ltr.publish_training(&examples).await?;
        Ok(TrainResult {
            enqueued: true,
            examples: examples.len(),
            reason: None,
        })
    }

    async fn build_examples(self: &Arc<Self>) -> AppResult<Vec<LtrExample>> {
        let since = Utc::now().naive_utc() - chrono::Duration::days(TRAIN_WINDOW_DAYS);
        let user_counts: Vec<(String, i64)> = sqlx::query_as(
            "SELECT sc_user_id, COUNT(*)::int8 AS n FROM user_events \
             WHERE event_type = ANY($1) AND created_at >= $2 \
             GROUP BY sc_user_id \
             HAVING COUNT(*) >= $3 \
             ORDER BY n DESC LIMIT $4",
        )
        .bind(POSITIVE_TYPES)
        .bind(since)
        .bind(MIN_POSITIVE_PER_USER)
        .bind(MAX_USERS)
        .fetch_all(&self.pg)
        .await?;

        if user_counts.is_empty() {
            warn!("[ltr.train] no active users");
            return Ok(Vec::new());
        }

        let mut out: Vec<LtrExample> = Vec::new();
        let mut group_counter: u64 = 0;
        for (sc_user_id, _n) in user_counts {
            let examples = self
                .build_user_examples(&sc_user_id, group_counter)
                .await
                .unwrap_or_default();
            if examples.len() >= 2 {
                out.extend(examples);
                group_counter += 1;
            }
        }
        Ok(out)
    }

    async fn build_user_examples(
        self: &Arc<Self>,
        sc_user_id: &str,
        group: u64,
    ) -> AppResult<Vec<LtrExample>> {
        let events: Vec<(String, String)> = sqlx::query_as(
            "SELECT sc_track_id, event_type FROM user_events \
             WHERE sc_user_id = $1 AND event_type = ANY($2)",
        )
        .bind(sc_user_id)
        .bind(ALL_TYPES)
        .fetch_all(&self.pg)
        .await?;

        let mut label_by_track: HashMap<String, i32> = HashMap::new();
        for (track_id, ev_type) in events {
            let Some(lab) = label_for(&ev_type) else {
                continue;
            };
            label_by_track
                .entry(track_id)
                .and_modify(|prev| {
                    if lab > *prev {
                        *prev = lab;
                    }
                })
                .or_insert(lab);
        }
        if label_by_track.len() < 3 {
            return Ok(Vec::new());
        }
        let has_negatives = label_by_track.values().any(|v| *v == 0);
        if !has_negatives {
            let randoms: Vec<(String,)> = sqlx::query_as(
                "SELECT sc_track_id FROM indexed_tracks \
                 WHERE indexed_at IS NOT NULL \
                 ORDER BY RANDOM() LIMIT $1",
            )
            .bind(MIN_NEGATIVES_PER_USER)
            .fetch_all(&self.pg)
            .await?;
            for (id,) in randoms {
                label_by_track.entry(id).or_insert(0);
            }
        }

        let user_collab = self.collab.get_user_vector(sc_user_id).await?;
        let user_taste_id = user_id_to_qdrant_id(sc_user_id);
        let mert_fut = self.retrieve_single(collections::USER_TASTE_MERT, user_taste_id);
        let clap_fut = self.retrieve_single(collections::USER_TASTE_CLAP, user_taste_id);
        let lyrics_fut = self.retrieve_single(collections::USER_TASTE_LYRICS, user_taste_id);
        let (user_mert, user_clap, user_lyrics) = tokio::join!(mert_fut, clap_fut, lyrics_fut);

        let track_ids: Vec<String> = label_by_track
            .keys()
            .take(MAX_PAIRS_PER_USER)
            .cloned()
            .collect();
        let numeric_ids: Vec<u64> = track_ids
            .iter()
            .filter_map(|id| id.parse::<u64>().ok())
            .collect();
        if numeric_ids.is_empty() {
            return Ok(Vec::new());
        }

        let track_vecs = self.load_track_vectors(&numeric_ids).await?;
        if track_vecs.is_empty() {
            return Ok(Vec::new());
        }

        let user_langs = self.detect_user_languages(sc_user_id).await?;
        let c_mert = self.centroids.get(collections::TRACKS_MERT);
        let c_clap = self.centroids.get(collections::TRACKS_CLAP);

        let mut examples = Vec::new();
        for id in &track_ids {
            let v = match track_vecs.get(id) {
                Some(v) => v,
                None => continue,
            };
            let label = *label_by_track.get(id).unwrap_or(&0);
            let mut feats = vec![0f32; LTR_FEATURE_COUNT];
            feats[0] = match (&user_collab, &v.collab) {
                (Some(u), Some(t)) => cosine(t, u),
                _ => 0.0,
            };
            feats[1] = match (&user_mert, &v.mert) {
                (Some(u), Some(t)) => whitened_cos(t, u, c_mert.as_deref()),
                _ => 0.0,
            };
            feats[2] = match (&user_clap, &v.clap) {
                (Some(u), Some(t)) => whitened_cos(t, u, c_clap.as_deref()),
                _ => 0.0,
            };
            feats[3] = match (&user_lyrics, &v.lyrics) {
                (Some(u), Some(t)) => cosine(t, u),
                _ => 0.0,
            };
            feats[4] = (v.playback_count.max(0) as f64).ln_1p() as f32;
            feats[5] = match &v.language {
                Some(l) if user_langs.contains(l) => 1.0,
                _ => 0.0,
            };
            examples.push(LtrExample {
                group,
                label,
                features: feats,
            });
        }
        Ok(examples)
    }

    async fn detect_user_languages(&self, sc_user_id: &str) -> AppResult<HashSet<String>> {
        let events: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events \
             WHERE sc_user_id = $1 AND event_type = ANY($2) \
             ORDER BY created_at DESC LIMIT 50",
        )
        .bind(sc_user_id)
        .bind(POSITIVE_TYPES)
        .fetch_all(&self.pg)
        .await?;
        if events.is_empty() {
            return Ok(HashSet::new());
        }
        let ids: Vec<String> = events.into_iter().map(|(s,)| s).collect();
        let tracks: Vec<(Option<String>,)> =
            sqlx::query_as("SELECT language FROM indexed_tracks WHERE sc_track_id = ANY($1)")
                .bind(&ids)
                .fetch_all(&self.pg)
                .await?;
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (l,) in tracks {
            if let Some(l) = l {
                *counts.entry(l).or_insert(0) += 1;
            }
        }
        let mut sorted: Vec<(String, usize)> = counts.into_iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(&a.1));
        Ok(sorted.into_iter().take(3).map(|(l, _)| l).collect())
    }

    async fn load_track_vectors(&self, ids: &[u64]) -> AppResult<HashMap<String, TrackVectors>> {
        let id_strs: Vec<String> = ids.iter().map(|i| i.to_string()).collect();

        let mert_fut = self.retrieve_batch(collections::TRACKS_MERT, ids);
        let clap_fut = self.retrieve_batch(collections::TRACKS_CLAP, ids);
        let lyrics_fut = self.retrieve_batch(collections::TRACKS_LYRICS, ids);
        let collab_fut = async { self.collab.get_track_vectors(ids).await };
        let meta_fut = async {
            sqlx::query_as::<_, (String, Option<Value>, Option<String>)>(
                "SELECT sc_track_id, raw_sc_data, language FROM indexed_tracks \
                 WHERE sc_track_id = ANY($1)",
            )
            .bind(&id_strs)
            .fetch_all(&self.pg)
            .await
        };
        let (mert_map, clap_map, lyrics_map, collab_map, tracks) =
            tokio::join!(mert_fut, clap_fut, lyrics_fut, collab_fut, meta_fut);

        let tracks = tracks?;
        let mut meta: HashMap<String, (Option<Value>, Option<String>)> = HashMap::new();
        for (id, raw, lang) in tracks {
            meta.insert(id, (raw, lang));
        }
        let mut out: HashMap<String, TrackVectors> = HashMap::new();
        for id in ids {
            let key = id.to_string();
            let (raw, lang) = meta.remove(&key).unwrap_or((None, None));
            let playback = raw
                .as_ref()
                .and_then(|v| v.get("playback_count"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            out.insert(
                key.clone(),
                TrackVectors {
                    collab: collab_map.get(&key).cloned(),
                    mert: mert_map.get(&key).cloned(),
                    clap: clap_map.get(&key).cloned(),
                    lyrics: lyrics_map.get(&key).cloned(),
                    playback_count: playback,
                    language: lang,
                },
            );
        }
        Ok(out)
    }

    async fn retrieve_single(&self, coll: &str, id: u64) -> Option<Vec<f32>> {
        let resp = self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(coll, vec![numeric_id(id)]).with_vectors(true))
            .await
            .ok()?;
        let p = resp.result.first()?;
        match p.vectors.as_ref().and_then(|v| v.vectors_options.as_ref()) {
            Some(VectorsOptions::Vector(VectorOutput { data, .. })) => Some(data.clone()),
            _ => None,
        }
    }

    async fn retrieve_batch(&self, coll: &str, ids: &[u64]) -> HashMap<String, Vec<f32>> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let pids: Vec<PointId> = ids.iter().copied().map(numeric_id).collect();
        let resp = match self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(coll, pids).with_vectors(true))
            .await
        {
            Ok(r) => r,
            Err(e) => {
                debug!(coll, error = %e, "retrieveBatch failed");
                return out;
            }
        };
        for p in resp.result {
            let id_str = match p.id.and_then(|id| id.point_id_options) {
                Some(PointIdOptions::Num(n)) => n.to_string(),
                Some(PointIdOptions::Uuid(u)) => u,
                None => continue,
            };
            if let Some(vectors) = p.vectors {
                if let Some(VectorsOptions::Vector(VectorOutput { data, .. })) =
                    vectors.vectors_options
                {
                    out.insert(id_str, data);
                }
            }
        }
        out
    }
}

fn whitened_cos(a: &[f32], b: &[f32], centroid: Option<&[f32]>) -> f32 {
    match centroid {
        None => cosine(a, b),
        Some(c) => {
            let n = a.len().min(b.len()).min(c.len());
            let mut aw = Vec::with_capacity(n);
            let mut bw = Vec::with_capacity(n);
            for i in 0..n {
                aw.push(a[i] - c[i]);
                bw.push(b[i] - c[i]);
            }
            cosine(&aw, &bw)
        }
    }
}

fn numeric_id(id: u64) -> PointId {
    PointId {
        point_id_options: Some(PointIdOptions::Num(id)),
    }
}

fn should_run_weekly() -> bool {
    let now = Utc::now();
    use chrono::{Datelike, Timelike};
    now.weekday() == chrono::Weekday::Sun && now.hour() == 4
}
