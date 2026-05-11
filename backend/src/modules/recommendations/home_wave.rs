use std::collections::{HashMap, HashSet};

use chrono::Utc;
use serde_json::Value;
use tracing::info;
use uuid::Uuid;

use crate::error::AppResult;
use crate::qdrant::collections;

use super::bandits;
use super::clusters::{
    recommend_id_str, Cluster, ClusterBuilder, ClusterNeighbor, ClusterResponse,
};
use super::debias::ips_debias;
use super::impressions::{log_clusters_async, ImpressionSource};
use super::quality;
use super::rerank_multi::RerankOptions;
use super::service::util::value_to_u64;
use super::service::{RecommendResult, RecommendationsService};
use super::sessions::mix_centroids;
use super::signal::{load_user_signals, SeedKind};
use super::taste_modes::TasteMode;

const ALL_CLUSTERS: &[&str] = &[
    "for_you",
    "top_artists",
    "adjacent",
    "fresh_drops",
    "same_vibe",
    "deep_cuts",
];

const POOL_FOR_VIBE_DEEP: usize = 320;
const NEIGHBORS_TOP_LIMIT: i64 = 8;
const NEIGHBORS_ADJ_LIMIT: i64 = 10;
const FRESH_DROP_LIMIT: i64 = 14;

pub struct HomeRequest {
    pub sc_user_id: String,
    pub languages: Option<Vec<String>>,
    pub per_cluster: usize,
}

#[derive(Debug, sqlx::FromRow)]
struct ArtistTrackRow {
    artist_id: Uuid,
    artist_name: String,
    avatar_url: Option<String>,
    sc_track_id: String,
}

impl RecommendationsService {
    pub async fn home_wave(&self, req: HomeRequest) -> AppResult<ClusterResponse> {
        let per_cluster = req.per_cluster.clamp(4, 24);
        let sc_user_id = req.sc_user_id.clone();
        let languages_vec = req.languages.clone();
        let languages = languages_vec.as_deref();

        let signals = load_user_signals(&self.pg, &sc_user_id).await?;

        if matches!(signals.best_seed_kind(), SeedKind::ColdStart) && !signals.has_any_signal() {
            return self.cold_start_response(languages, per_cluster, &sc_user_id).await;
        }

        let exclude_set: HashSet<String> = signals
            .played
            .iter()
            .chain(signals.disliked_ids.iter())
            .cloned()
            .collect();
        let exclude_vec: Vec<String> = exclude_set.iter().cloned().collect();

        let seeds = signals.positive_seed();
        let taste_modes_fut = self.build_taste_modes(&seeds);
        let session_fut = self.detect_current_session(&sc_user_id);
        let hour_fut = self.hour_context(&sc_user_id, Utc::now());
        let anti_fut = self.build_anti_centroid_from_negatives(&signals.negatives);
        let bandits_fut = bandits::load_stats(&self.pg, &sc_user_id);

        let (taste_modes, session_ctx, hour_ctx, anti_centroid, bandit_stats) = tokio::join!(
            taste_modes_fut,
            session_fut,
            hour_fut,
            anti_fut,
            bandits_fut,
        );
        let session_ctx = session_ctx.unwrap_or(None);
        let hour_ctx = hour_ctx.unwrap_or(None);
        let bandit_stats = bandit_stats.unwrap_or_default();

        let overall_centroid = if let Some(first) = taste_modes.first() {
            Some(first.centroid.clone())
        } else {
            None
        };
        let mixed_for_search = mix_centroids(
            overall_centroid.as_deref(),
            session_ctx.as_ref().map(|s| s.centroid.as_slice()),
            hour_ctx.as_ref().map(|h| h.centroid.as_slice()),
        );

        let recent_artists = self.recent_artists(&sc_user_id, 40).await.unwrap_or_default();

        let mut builder = ClusterBuilder::new();
        builder.reserve(exclude_vec.iter().cloned());

        let (for_you_ids, for_you_features) = self
            .build_for_you_cluster(
                &taste_modes,
                &exclude_vec,
                languages,
                anti_centroid.as_deref(),
                &recent_artists,
                per_cluster,
            )
            .await;
        for (id, feats) in for_you_features {
            builder.attach_features(id, feats);
        }
        builder.push("for_you", for_you_ids);

        let top_artists = self
            .load_top_artists_cluster(&sc_user_id, builder.taken(), NEIGHBORS_TOP_LIMIT)
            .await;
        builder.push_with_neighbors(
            "top_artists",
            dedupe_neighbors(top_artists, builder.taken(), per_cluster),
        );

        let adjacent = self
            .load_adjacent_artists_cluster(&sc_user_id, builder.taken(), NEIGHBORS_ADJ_LIMIT)
            .await;
        builder.push_with_neighbors(
            "adjacent",
            dedupe_neighbors(adjacent, builder.taken(), per_cluster),
        );

        let fresh = self
            .load_fresh_drops(&sc_user_id, builder.taken(), FRESH_DROP_LIMIT)
            .await;
        builder.push(
            "fresh_drops",
            fresh.into_iter()
                .filter(|id| !builder.taken().contains(id))
                .take(per_cluster)
                .collect(),
        );

        let (vibe_ids, deep_ids) = match mixed_for_search.as_deref() {
            Some(centroid) => {
                self.build_vibe_and_deep(
                    centroid,
                    &exclude_vec,
                    languages,
                    builder.taken(),
                    per_cluster,
                    anti_centroid.as_deref(),
                    &recent_artists,
                    overall_centroid.as_deref(),
                )
                .await
            }
            None => (Vec::new(), Vec::new()),
        };
        builder.push("same_vibe", vibe_ids);
        builder.push("deep_cuts", deep_ids);

        self.apply_quality_filter(&mut builder).await;

        let missing = self
            .s3
            .find_missing(&builder.all_track_ids())
            .await
            .unwrap_or_default();
        builder.drop_missing(&missing);

        let features_map = builder.features_map().clone();
        let mut response = builder.finish();
        reorder_by_bandits(&mut response.clusters, &bandit_stats);

        let counts: Vec<(String, i64)> = response
            .clusters
            .iter()
            .map(|c| (c.id.to_string(), c.track_ids.len() as i64))
            .collect();
        if !counts.is_empty() {
            let pg = self.pg.clone();
            let user = sc_user_id.clone();
            tokio::spawn(async move {
                let _ = bandits::record_shows(&pg, &user, &counts).await;
            });
        }

        log_clusters_async(
            self.pg.clone(),
            sc_user_id.clone(),
            ImpressionSource::Home,
            &response.clusters,
            &features_map,
        );

        info!(
            user = %sc_user_id,
            clusters = response.clusters.len(),
            modes = taste_modes.len(),
            session = session_ctx.is_some(),
            hour = hour_ctx.is_some(),
            "home_wave built"
        );
        Ok(response)
    }

    async fn cold_start_response(
        &self,
        languages: Option<&[String]>,
        per_cluster: usize,
        sc_user_id: &str,
    ) -> AppResult<ClusterResponse> {
        let pool = self.cold_start_pool(languages, per_cluster * 4).await?;
        let mut builder = ClusterBuilder::new();
        builder.push("discover", pool.into_iter().take(per_cluster).collect());
        self.apply_quality_filter(&mut builder).await;
        let missing = self
            .s3
            .find_missing(&builder.all_track_ids())
            .await
            .unwrap_or_default();
        builder.drop_missing(&missing);
        let features_map = builder.features_map().clone();
        let response = builder.finish();
        log_clusters_async(
            self.pg.clone(),
            sc_user_id.to_string(),
            ImpressionSource::Home,
            &response.clusters,
            &features_map,
        );
        Ok(response)
    }

    async fn build_for_you_cluster(
        &self,
        modes: &[TasteMode],
        exclude: &[String],
        languages: Option<&[String]>,
        anti_centroid: Option<&[f32]>,
        recent_artists: &HashSet<String>,
        per_cluster: usize,
    ) -> (Vec<String>, HashMap<String, Vec<f32>>) {
        if modes.is_empty() {
            return (Vec::new(), HashMap::new());
        }
        let filter = self.build_filter(exclude, languages);
        let per_mode = (per_cluster.div_ceil(modes.len())) + 2;
        let pool_per_mode = (per_mode * 6).max(40);

        let mut futures = Vec::new();
        for mode in modes {
            let centroid = mode.centroid.clone();
            let filter_clone = filter.clone();
            futures.push(async move {
                self.search_by_vector(
                    collections::TRACKS_MERT,
                    &centroid,
                    filter_clone.as_ref(),
                    pool_per_mode,
                )
                .await
            });
        }
        let results = futures::future::join_all(futures).await;

        let mut per_mode_ids: Vec<Vec<RecommendResult>> = results;

        if let Some(anti) = anti_centroid {
            for vec in per_mode_ids.iter_mut() {
                let drop = self.filter_against_anti(vec, anti).await;
                *vec = drop;
            }
        }

        let mut taken = HashSet::<String>::new();
        for id in exclude {
            taken.insert(id.clone());
        }

        let mut out = Vec::with_capacity(per_cluster);
        let mut features: HashMap<String, Vec<f32>> = HashMap::new();
        let mut mode_cursors = vec![0usize; per_mode_ids.len()];
        while out.len() < per_cluster {
            let mut advanced = false;
            for (mi, results) in per_mode_ids.iter().enumerate() {
                if out.len() >= per_cluster {
                    break;
                }
                while mode_cursors[mi] < results.len() {
                    let candidate = &results[mode_cursors[mi]];
                    mode_cursors[mi] += 1;
                    let id = recommend_id_str(&candidate.id);
                    if id.is_empty() || taken.contains(&id) {
                        continue;
                    }
                    taken.insert(id.clone());
                    features.insert(
                        id.clone(),
                        for_you_features(candidate, mi as f32, recent_artists),
                    );
                    out.push(id);
                    advanced = true;
                    break;
                }
            }
            if !advanced {
                break;
            }
        }
        (out, features)
    }

    async fn build_vibe_and_deep(
        &self,
        centroid: &[f32],
        exclude: &[String],
        languages: Option<&[String]>,
        taken: &HashSet<String>,
        per_cluster: usize,
        anti_centroid: Option<&[f32]>,
        recent_artists: &HashSet<String>,
        user_centroid: Option<&[f32]>,
    ) -> (Vec<String>, Vec<String>) {
        let filter = self.build_filter(exclude, languages);
        let mut pool = self
            .search_by_vector(
                collections::TRACKS_MERT,
                centroid,
                filter.as_ref(),
                POOL_FOR_VIBE_DEEP,
            )
            .await;
        if pool.is_empty() {
            return (Vec::new(), Vec::new());
        }
        self.attach_playback_counts(&mut pool).await;
        ips_debias(&mut pool);

        let vibe_pool: Vec<RecommendResult> = pool
            .iter()
            .filter(|r| !taken.contains(&recommend_id_str(&r.id)))
            .cloned()
            .collect();

        let vibe_ranked = self
            .rerank_multi(
                vibe_pool,
                RerankOptions {
                    limit: per_cluster,
                    diversity: 0.35,
                    novelty: 0.15,
                    serendipity: 0.05,
                    anti_centroid: anti_centroid.map(|a| a.to_vec()),
                    recent_artists: recent_artists.clone(),
                    user_centroid: user_centroid.map(|v| v.to_vec()),
                },
            )
            .await;
        let vibe_ids: Vec<String> = vibe_ranked
            .iter()
            .take(per_cluster)
            .map(|r| recommend_id_str(&r.id))
            .collect();
        let vibe_set: HashSet<String> = vibe_ids.iter().cloned().collect();

        let deep_pool: Vec<RecommendResult> = pool
            .into_iter()
            .filter(|r| {
                let id = recommend_id_str(&r.id);
                !taken.contains(&id) && !vibe_set.contains(&id)
            })
            .collect();

        let deep_ranked = self
            .rerank_multi(
                deep_pool,
                RerankOptions {
                    limit: per_cluster,
                    diversity: 0.55,
                    novelty: 0.25,
                    serendipity: 0.20,
                    anti_centroid: anti_centroid.map(|a| a.to_vec()),
                    recent_artists: recent_artists.clone(),
                    user_centroid: user_centroid.map(|v| v.to_vec()),
                },
            )
            .await;
        let deep_ids: Vec<String> = deep_ranked
            .iter()
            .take(per_cluster)
            .map(|r| recommend_id_str(&r.id))
            .collect();

        (vibe_ids, deep_ids)
    }

    async fn filter_against_anti(
        &self,
        pool: &[RecommendResult],
        anti: &[f32],
    ) -> Vec<RecommendResult> {
        let numeric: Vec<u64> = pool.iter().filter_map(|r| value_to_u64(&r.id)).collect();
        if numeric.is_empty() {
            return pool.to_vec();
        }
        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric)
            .await;
        pool.iter()
            .filter(|r| {
                let Some(n) = value_to_u64(&r.id) else {
                    return true;
                };
                match vec_map.get(&n.to_string()) {
                    Some(v) => crate::modules::centroids::cosine(v, anti) < 0.85,
                    None => true,
                }
            })
            .cloned()
            .collect()
    }

    async fn build_anti_centroid_from_negatives(
        &self,
        negatives: &[super::signal::WeightedTrack],
    ) -> Option<Vec<f32>> {
        if negatives.is_empty() {
            return None;
        }
        let numeric: Vec<u64> = negatives
            .iter()
            .filter_map(|n| n.sc_track_id.parse::<u64>().ok())
            .collect();
        if numeric.is_empty() {
            return None;
        }
        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric)
            .await;
        let weights: Vec<(String, f32)> = negatives
            .iter()
            .map(|n| (n.sc_track_id.clone(), n.weight.max(0.01)))
            .collect();
        super::taste_modes::build_anti_centroid(&vec_map, &weights)
    }

    async fn recent_artists(&self, sc_user_id: &str, limit: i64) -> AppResult<HashSet<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT LOWER(a.name)
             FROM user_events ue
             JOIN indexed_tracks it ON it.sc_track_id = ue.sc_track_id
             JOIN track_artists ta ON ta.indexed_track_id = it.id AND ta.role = 'primary'
             JOIN artists a ON a.id = ta.artist_id
             WHERE ue.sc_user_id = $1
               AND ue.created_at > NOW() - INTERVAL '7 days'
             ORDER BY ue.created_at DESC
             LIMIT $2",
        )
        .bind(sc_user_id)
        .bind(limit)
        .fetch_all(&self.pg)
        .await?;
        Ok(rows.into_iter().map(|(n,)| n).collect())
    }

    async fn load_top_artists_cluster(
        &self,
        sc_user_id: &str,
        exclude: &HashSet<String>,
        limit: i64,
    ) -> Vec<ClusterNeighbor> {
        let exclude_vec: Vec<String> = exclude.iter().cloned().collect();
        let rows: Vec<ArtistTrackRow> = match sqlx::query_as::<_, ArtistTrackRow>(
            "WITH top_artists AS (
                 SELECT ta.artist_id, COUNT(*) AS cnt
                 FROM user_events ue
                 JOIN indexed_tracks it ON it.sc_track_id = ue.sc_track_id
                 JOIN track_artists ta ON ta.indexed_track_id = it.id AND ta.role = 'primary'
                 WHERE ue.sc_user_id = $1
                   AND ue.event_type IN ('like', 'local_like', 'playlist_add')
                 GROUP BY ta.artist_id
                 ORDER BY cnt DESC
                 LIMIT $2
             ),
             ranked AS (
                 SELECT
                     ta.artist_id, it.sc_track_id,
                     ROW_NUMBER() OVER (
                         PARTITION BY ta.artist_id
                         ORDER BY
                             CASE WHEN it.sc_track_id = ANY($3) THEN 1 ELSE 0 END,
                             COALESCE(c.play_count, 0) DESC
                     ) AS rn
                 FROM top_artists tau
                 JOIN track_artists ta ON ta.artist_id = tau.artist_id AND ta.role = 'primary'
                 JOIN indexed_tracks it ON it.id = ta.indexed_track_id
                 LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                 WHERE it.indexed_at IS NOT NULL
             )
             SELECT a.id AS artist_id, a.name AS artist_name, a.avatar_url, r.sc_track_id
             FROM ranked r
             JOIN artists a ON a.id = r.artist_id
             WHERE r.rn = 1 AND a.merged_into IS NULL",
        )
        .bind(sc_user_id)
        .bind(limit)
        .bind(&exclude_vec)
        .fetch_all(&self.pg)
        .await
        {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        rows.into_iter()
            .map(|r| ClusterNeighbor {
                track_id: r.sc_track_id,
                artist_id: r.artist_id,
                artist_name: r.artist_name,
                avatar_url: r.avatar_url,
            })
            .collect()
    }

    async fn load_adjacent_artists_cluster(
        &self,
        sc_user_id: &str,
        exclude: &HashSet<String>,
        limit: i64,
    ) -> Vec<ClusterNeighbor> {
        let exclude_vec: Vec<String> = exclude.iter().cloned().collect();
        let rows: Vec<ArtistTrackRow> = match sqlx::query_as::<_, ArtistTrackRow>(
            "WITH user_artists AS (
                 SELECT DISTINCT ta.artist_id
                 FROM user_events ue
                 JOIN indexed_tracks it ON it.sc_track_id = ue.sc_track_id
                 JOIN track_artists ta ON ta.indexed_track_id = it.id AND ta.role = 'primary'
                 WHERE ue.sc_user_id = $1
                   AND ue.event_type IN ('like', 'local_like', 'playlist_add')
                 LIMIT 80
             ),
             co AS (
                 SELECT
                     (CASE WHEN ac.a_id IN (SELECT artist_id FROM user_artists)
                           THEN ac.b_id ELSE ac.a_id END) AS co_id,
                     MAX(ac.weight) AS w
                 FROM artist_coplay ac
                 WHERE (ac.a_id IN (SELECT artist_id FROM user_artists)
                     OR ac.b_id IN (SELECT artist_id FROM user_artists))
                   AND NOT (
                       ac.a_id IN (SELECT artist_id FROM user_artists)
                       AND ac.b_id IN (SELECT artist_id FROM user_artists)
                   )
                 GROUP BY co_id
                 ORDER BY w DESC
                 LIMIT $2
             ),
             ranked AS (
                 SELECT
                     ta.artist_id, it.sc_track_id,
                     ROW_NUMBER() OVER (
                         PARTITION BY ta.artist_id
                         ORDER BY
                             CASE WHEN it.sc_track_id = ANY($3) THEN 1 ELSE 0 END,
                             COALESCE(c.play_count, 0) DESC
                     ) AS rn,
                     co.w
                 FROM co
                 JOIN track_artists ta ON ta.artist_id = co.co_id AND ta.role = 'primary'
                 JOIN indexed_tracks it ON it.id = ta.indexed_track_id
                 LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                 WHERE it.indexed_at IS NOT NULL
             )
             SELECT a.id AS artist_id, a.name AS artist_name, a.avatar_url, r.sc_track_id
             FROM ranked r
             JOIN artists a ON a.id = r.artist_id
             WHERE r.rn = 1 AND a.merged_into IS NULL
             ORDER BY r.w DESC NULLS LAST",
        )
        .bind(sc_user_id)
        .bind(limit)
        .bind(&exclude_vec)
        .fetch_all(&self.pg)
        .await
        {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        rows.into_iter()
            .map(|r| ClusterNeighbor {
                track_id: r.sc_track_id,
                artist_id: r.artist_id,
                artist_name: r.artist_name,
                avatar_url: r.avatar_url,
            })
            .collect()
    }

    async fn load_fresh_drops(
        &self,
        sc_user_id: &str,
        exclude: &HashSet<String>,
        limit: i64,
    ) -> Vec<String> {
        let exclude_vec: Vec<String> = exclude.iter().cloned().collect();
        let rows: Vec<(String,)> = match sqlx::query_as(
            "WITH user_artists AS (
                 SELECT DISTINCT ta.artist_id
                 FROM user_events ue
                 JOIN indexed_tracks it ON it.sc_track_id = ue.sc_track_id
                 JOIN track_artists ta ON ta.indexed_track_id = it.id AND ta.role = 'primary'
                 WHERE ue.sc_user_id = $1
                   AND ue.event_type IN ('like', 'local_like')
                   AND ue.created_at > NOW() - INTERVAL '120 days'
             )
             SELECT it.sc_track_id
             FROM track_artists ta
             JOIN indexed_tracks it ON it.id = ta.indexed_track_id
             WHERE ta.artist_id IN (SELECT artist_id FROM user_artists)
               AND ta.role = 'primary'
               AND it.indexed_at IS NOT NULL
               AND it.indexed_at > NOW() - INTERVAL '30 days'
               AND NOT (it.sc_track_id = ANY($2))
             ORDER BY it.indexed_at DESC
             LIMIT $3",
        )
        .bind(sc_user_id)
        .bind(&exclude_vec)
        .bind(limit)
        .fetch_all(&self.pg)
        .await
        {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        rows.into_iter().map(|(id,)| id).collect()
    }

    async fn apply_quality_filter(&self, builder: &mut ClusterBuilder) {
        const QUALITY_THRESHOLD: f32 = 0.4;
        let all_ids = builder.all_track_ids();
        if all_ids.is_empty() {
            return;
        }
        let rows: Vec<(String, Option<Value>, Option<i64>, Option<f32>)> = sqlx::query_as(
            "SELECT it.sc_track_id, it.raw_sc_data, c.play_count, it.quality_score
             FROM indexed_tracks it
             LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
             WHERE it.sc_track_id = ANY($1)",
        )
        .bind(&all_ids)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();

        let by_id: HashMap<String, (Option<Value>, i64, Option<f32>)> = rows
            .into_iter()
            .map(|(id, raw, plays, q)| (id, (raw, plays.unwrap_or(0), q)))
            .collect();

        let to_drop: HashSet<String> = all_ids
            .into_iter()
            .filter(|id| {
                let Some((raw, plays, quality)) = by_id.get(id) else {
                    return true;
                };
                if let Some(q) = quality {
                    return *q < QUALITY_THRESHOLD;
                }
                !quality::passes(raw.as_ref(), *plays, quality::MIN_PLAYS_DEFAULT)
            })
            .collect();
        builder.drop_missing(&to_drop);
    }
}

fn for_you_features(
    candidate: &RecommendResult,
    mode_index: f32,
    recent_artists: &HashSet<String>,
) -> Vec<f32> {
    let score = candidate.score.unwrap_or(0.0);
    let log_plays = match candidate.playback_count {
        Some(p) if p > 0 => ((p as f64).ln_1p() as f32) / 16.0,
        _ => 0.0,
    };
    let novelty = match candidate.artist.as_deref() {
        Some(a) if !a.is_empty() && !recent_artists.contains(&a.to_lowercase()) => 1.0,
        Some(_) => 0.0,
        None => 0.5,
    };
    vec![score, mode_index, 0.0, 0.0, log_plays, 0.0, novelty, 0.0]
}

fn dedupe_neighbors(
    raw: Vec<ClusterNeighbor>,
    taken: &HashSet<String>,
    limit: usize,
) -> Vec<ClusterNeighbor> {
    let mut out = Vec::with_capacity(limit);
    let mut seen_artists: HashSet<Uuid> = HashSet::new();
    for n in raw {
        if out.len() >= limit {
            break;
        }
        if taken.contains(&n.track_id) {
            continue;
        }
        if !seen_artists.insert(n.artist_id) {
            continue;
        }
        out.push(n);
    }
    out
}

fn reorder_by_bandits(
    clusters: &mut Vec<Cluster>,
    stats: &HashMap<String, bandits::ClusterStat>,
) {
    if clusters.len() <= 1 {
        return;
    }
    let order: Vec<&str> = bandits::order_by_thompson(ALL_CLUSTERS, stats);
    let priority: HashMap<&str, usize> =
        order.into_iter().enumerate().map(|(i, c)| (c, i)).collect();
    clusters.sort_by_key(|c| priority.get(c.id).copied().unwrap_or(usize::MAX));
}

impl RecommendationsService {
    async fn attach_playback_counts(&self, pool: &mut [RecommendResult]) {
        if pool.is_empty() {
            return;
        }
        let ids: Vec<String> = pool
            .iter()
            .map(|r| recommend_id_str(&r.id))
            .filter(|s| !s.is_empty())
            .collect();
        if ids.is_empty() {
            return;
        }
        let rows: Vec<(String, Option<i64>)> = sqlx::query_as(
            "SELECT sc_track_id, play_count FROM sc_track_counters WHERE sc_track_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();
        let by_id: HashMap<String, i64> = rows
            .into_iter()
            .map(|(id, p)| (id, p.unwrap_or(0)))
            .collect();
        for r in pool.iter_mut() {
            let id = recommend_id_str(&r.id);
            if let Some(p) = by_id.get(&id) {
                r.playback_count = Some(*p);
            }
        }
    }
}
