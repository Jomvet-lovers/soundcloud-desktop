use std::collections::HashMap;

use tracing::{info, warn};
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::centroids::{cosine, normalize};
use crate::qdrant::collections;

use super::clusters::{ClusterBuilder, ClusterNeighbor, ClusterResponse};
use super::home_wave::merge_audio_pools;
use super::mmr::{greedy_pick, max_cosine_to_selected};
use super::service::RecommendationsService;
use super::smart_wave::{self, SmartWaveSeed};

const RELATED_LIMIT: i64 = 20;
const PER_NEIGHBOR_PROBE: i64 = 8;
const POOL_FOR_VIBE: usize = 120;
const POOL_FOR_DEEP: usize = 240;
const ARTIST_TOP_TRACKS: i64 = 30;
const WAVE_LIMIT: usize = 24;

#[derive(Debug, sqlx::FromRow)]
struct RelatedArtistRow {
    id: Uuid,
    name: String,
    avatar_url: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct NeighborTrackRow {
    artist_id: Uuid,
    sc_track_id: String,
}

impl RecommendationsService {
    pub async fn artist_wave(
        &self,
        artist_id: Uuid,
        sc_user_id: &str,
        per_cluster: usize,
    ) -> AppResult<ClusterResponse> {
        let per_cluster = per_cluster.clamp(4, 24);

        let top_tracks = self
            .load_artist_top_tracks(artist_id, ARTIST_TOP_TRACKS)
            .await?;
        if top_tracks.is_empty() {
            return Ok(ClusterBuilder::new().finish());
        }

        let top_ids: Vec<u64> = top_tracks
            .iter()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        let mert_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &top_ids)
            .await;
        let clap_map = self
            .retrieve_vectors(collections::TRACKS_CLAP, &top_ids)
            .await;
        let lyrics_map = self
            .retrieve_vectors(collections::TRACKS_LYRICS, &top_ids)
            .await;
        let centroid = compute_centroid(&top_ids, &mert_map);
        let clap_centroid = compute_centroid(&top_ids, &clap_map);
        let lyrics_centroid = compute_centroid(&top_ids, &lyrics_map);

        let related = self.load_related_artists(artist_id, RELATED_LIMIT).await?;

        let exclude_artist: Vec<String> = top_tracks.to_vec();
        let filter = self.build_filter(&exclude_artist, None);

        let wave_fut = smart_wave::cluster_track_ids(
            self,
            sc_user_id,
            None,
            SmartWaveSeed::Artist(artist_id, &top_ids),
            WAVE_LIMIT,
        );

        let vibe_fut = async {
            let mert_fut = async {
                if let Some(c) = &centroid {
                    self.search_by_vector(
                        collections::TRACKS_MERT,
                        c,
                        filter.as_ref(),
                        POOL_FOR_VIBE,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let clap_fut = async {
                if let Some(c) = &clap_centroid {
                    self.search_by_vector(
                        collections::TRACKS_CLAP,
                        c,
                        filter.as_ref(),
                        POOL_FOR_VIBE / 2,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let lyrics_fut = async {
                if let Some(c) = &lyrics_centroid {
                    self.search_by_vector(
                        collections::TRACKS_LYRICS,
                        c,
                        filter.as_ref(),
                        POOL_FOR_VIBE / 2,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let (mert_pool, clap_pool, lyrics_pool) = tokio::join!(mert_fut, clap_fut, lyrics_fut);
            merge_audio_pools(&mert_pool, &clap_pool, &lyrics_pool)
        };

        let neighbors_fut = self.build_neighbors_cluster(&related, centroid.as_deref());

        let deep_fut = async {
            let mert_fut = async {
                if let Some(c) = &centroid {
                    self.search_by_vector(
                        collections::TRACKS_MERT,
                        c,
                        filter.as_ref(),
                        POOL_FOR_DEEP,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let clap_fut = async {
                if let Some(c) = &clap_centroid {
                    self.search_by_vector(
                        collections::TRACKS_CLAP,
                        c,
                        filter.as_ref(),
                        POOL_FOR_DEEP / 2,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let lyrics_fut = async {
                if let Some(c) = &lyrics_centroid {
                    self.search_by_vector(
                        collections::TRACKS_LYRICS,
                        c,
                        filter.as_ref(),
                        POOL_FOR_DEEP / 2,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let (mert_pool, clap_pool, lyrics_pool) = tokio::join!(mert_fut, clap_fut, lyrics_fut);
            merge_audio_pools(&mert_pool, &clap_pool, &lyrics_pool)
        };

        let (wave_ids, vibe_pool, neighbors_raw, deep_pool) =
            tokio::join!(wave_fut, vibe_fut, neighbors_fut, deep_fut);

        let mut builder = ClusterBuilder::new();
        builder.reserve(top_tracks.iter().cloned());
        builder.push("wave", wave_ids);

        let essence_ids: Vec<String> = top_tracks.iter().take(per_cluster).cloned().collect();
        builder.push("essence", essence_ids);

        let vibe_ids = super::clusters::pick_unique_ids(&vibe_pool, builder.taken(), per_cluster);
        builder.push("vibe", vibe_ids);

        let filtered_neighbors: Vec<ClusterNeighbor> = neighbors_raw
            .into_iter()
            .filter(|n| !builder.taken().contains(&n.track_id))
            .take(per_cluster)
            .collect();
        builder.push_with_neighbors("neighbors", filtered_neighbors);

        let deep_ids = build_deep_cluster(
            &deep_pool,
            centroid.as_deref(),
            self,
            builder.taken(),
            per_cluster,
        )
        .await;
        builder.push("deep", deep_ids);

        let missing = self
            .s3
            .find_missing(&builder.all_track_ids())
            .await
            .unwrap_or_default();
        builder.drop_missing(&missing);

        info!(
            artist = %artist_id,
            clusters = builder.taken().len(),
            "artist_wave built"
        );
        let response = builder.finish();
        super::impressions::log_clusters_async(
            self.pg.clone(),
            sc_user_id.to_string(),
            super::impressions::ImpressionSource::Artist,
            &response.clusters,
            &std::collections::HashMap::new(),
        );
        Ok(response)
    }

    async fn load_artist_top_tracks(&self, artist_id: Uuid, limit: i64) -> AppResult<Vec<String>> {
        let rows = sqlx::query_file_scalar!(
            "queries/recommendations/artist_wave/load_artist_top_tracks.sql",
            artist_id,
            limit
        )
        .fetch_all(&self.pg)
        .await?;
        Ok(rows)
    }

    /// Публичный helper для handlers::wave_artist — нужен seed-список треков
    /// артиста чтобы стартовать SmartWave прямо от него.
    pub async fn load_artist_top_track_ids(
        &self,
        artist_id: Uuid,
        limit: i64,
    ) -> AppResult<Vec<u64>> {
        let rows = self.load_artist_top_tracks(artist_id, limit).await?;
        Ok(rows
            .into_iter()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect())
    }

    async fn load_related_artists(
        &self,
        artist_id: Uuid,
        limit: i64,
    ) -> AppResult<Vec<RelatedArtistRow>> {
        let rows = sqlx::query_file!(
            "queries/recommendations/artist_wave/load_related_artists.sql",
            artist_id,
            limit
        )
        .fetch_all(&self.pg)
        .await?
        .into_iter()
        .map(|r| RelatedArtistRow {
            id: r.id,
            name: r.name,
            avatar_url: r.avatar_url,
        })
        .collect();
        Ok(rows)
    }

    async fn build_neighbors_cluster(
        &self,
        related: &[RelatedArtistRow],
        centroid: Option<&[f32]>,
    ) -> Vec<ClusterNeighbor> {
        if related.is_empty() {
            return Vec::new();
        }
        let centroid = match centroid {
            Some(c) => c,
            None => return Vec::new(),
        };
        let ids: Vec<Uuid> = related.iter().map(|r| r.id).collect();
        let limit = related.len() as i64 * PER_NEIGHBOR_PROBE;
        let rows: Vec<NeighborTrackRow> = match sqlx::query_file!(
            "queries/recommendations/artist_wave/load_neighbor_tracks.sql",
            &ids,
            limit
        )
        .fetch_all(&self.pg)
        .await
        {
            Ok(v) => v
                .into_iter()
                .map(|r| NeighborTrackRow {
                    artist_id: r.artist_id,
                    sc_track_id: r.sc_track_id,
                })
                .collect(),
            Err(e) => {
                warn!(error = %e, "artist_wave: neighbors query failed");
                return Vec::new();
            }
        };

        let mut by_artist: HashMap<Uuid, Vec<String>> = HashMap::new();
        for r in rows {
            by_artist
                .entry(r.artist_id)
                .or_default()
                .push(r.sc_track_id);
        }
        let candidate_track_ids: Vec<u64> = by_artist
            .values()
            .flatten()
            .filter_map(|s| s.parse::<u64>().ok())
            .collect();
        if candidate_track_ids.is_empty() {
            return Vec::new();
        }

        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &candidate_track_ids)
            .await;
        let mut out: Vec<ClusterNeighbor> = Vec::with_capacity(related.len());
        for ra in related {
            let track_ids = match by_artist.get(&ra.id) {
                Some(v) if !v.is_empty() => v,
                _ => continue,
            };
            let mut best: Option<(String, f32)> = None;
            for t_id in track_ids {
                let Some(v) = vec_map.get(t_id) else { continue };
                let s = cosine(centroid, v);
                if best.as_ref().map(|(_, b)| s > *b).unwrap_or(true) {
                    best = Some((t_id.clone(), s));
                }
            }
            if let Some((track_id, _)) = best {
                out.push(ClusterNeighbor {
                    track_id,
                    artist_id: ra.id,
                    artist_name: ra.name.clone(),
                    avatar_url: ra.avatar_url.clone(),
                });
            }
        }
        out
    }
}

fn compute_centroid(ids: &[u64], vec_map: &HashMap<String, Vec<f32>>) -> Option<Vec<f32>> {
    if ids.is_empty() {
        return None;
    }
    let mut sum: Option<Vec<f32>> = None;
    let mut count = 0usize;
    for id in ids {
        let Some(v) = vec_map.get(&id.to_string()) else {
            continue;
        };
        match sum.as_mut() {
            Some(acc) => {
                let n = acc.len().min(v.len());
                for i in 0..n {
                    acc[i] += v[i];
                }
            }
            None => sum = Some(v.clone()),
        }
        count += 1;
    }
    let mut acc = sum?;
    if count == 0 {
        return None;
    }
    let inv = 1.0 / count as f32;
    for x in acc.iter_mut() {
        *x *= inv;
    }
    normalize(&mut acc);
    Some(acc)
}

async fn build_deep_cluster(
    pool: &[super::service::RecommendResult],
    centroid: Option<&[f32]>,
    service: &RecommendationsService,
    taken: &std::collections::HashSet<String>,
    limit: usize,
) -> Vec<String> {
    if pool.is_empty() || limit == 0 {
        return Vec::new();
    }
    let candidates: Vec<(String, u64, f32)> = pool
        .iter()
        .filter_map(|r| {
            let id_str = super::clusters::recommend_id_str(&r.id);
            if id_str.is_empty() || taken.contains(&id_str) {
                return None;
            }
            let num = id_str.parse::<u64>().ok()?;
            Some((id_str, num, r.score.unwrap_or(0.0)))
        })
        .collect();
    if candidates.is_empty() {
        return Vec::new();
    }

    let centroid = match centroid {
        Some(c) => c,
        None => {
            return candidates
                .into_iter()
                .take(limit)
                .map(|(id, _, _)| id)
                .collect();
        }
    };

    let numeric_ids: Vec<u64> = candidates.iter().map(|(_, n, _)| *n).collect();
    let vec_map = service
        .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
        .await;

    let mut work: Vec<(String, Vec<f32>, f32)> = candidates
        .into_iter()
        .filter_map(|(id_str, num, rel)| {
            vec_map
                .get(&num.to_string())
                .map(|v| (id_str, v.clone(), rel))
        })
        .collect();
    if work.is_empty() {
        return Vec::new();
    }
    work.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let lambda = 0.55f32;
    let pool_vecs: Vec<Vec<f32>> = work.iter().map(|(_, v, _)| v.clone()).collect();
    let relevances: Vec<f32> = work.iter().map(|(_, _, r)| *r).collect();
    let centroid_owned = centroid.to_vec();
    let picks = greedy_pick(&pool_vecs, limit, |cand, selected, pool_vecs| {
        let rel_to_centroid = cosine(&pool_vecs[cand], &centroid_owned);
        let diversity_term = 1.0 - max_cosine_to_selected(cand, selected, pool_vecs);
        lambda * rel_to_centroid + (1.0 - lambda) * diversity_term + 0.1 * relevances[cand]
    });
    picks.into_iter().map(|i| work[i].0.clone()).collect()
}
