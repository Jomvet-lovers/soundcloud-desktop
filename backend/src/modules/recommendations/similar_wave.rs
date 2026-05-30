use std::collections::HashSet;

use tracing::info;
use uuid::Uuid;

use crate::error::AppResult;
use crate::modules::centroids::cosine;
use crate::qdrant::collections;

use super::clusters::{pick_unique_ids, ClusterBuilder, ClusterNeighbor, ClusterResponse};
use super::home_wave::merge_audio_pools;
use super::service::util::parse_id_or_null;
use super::service::RecommendationsService;
use super::smart_wave::{self, SmartWaveSeed};

const SAME_ARTIST_POOL: i64 = 60;
const FEATURED_LIMIT: i64 = 8;
const FANS_ALSO_LIMIT: usize = 120;
const SAME_VIBE_POOL: usize = 160;
const WAVE_LIMIT: usize = 24;

#[derive(Debug, sqlx::FromRow)]
struct ArtistTrackRow {
    artist_id: Uuid,
    artist_name: String,
    avatar_url: Option<String>,
    sc_track_id: String,
}

#[derive(Debug, sqlx::FromRow)]
struct PrimaryArtistRow {
    primary_artist_id: Uuid,
}

impl RecommendationsService {
    pub async fn similar_wave(
        &self,
        sc_track_id: &str,
        sc_user_id: &str,
        languages: Option<&[String]>,
        per_cluster: usize,
    ) -> AppResult<ClusterResponse> {
        let per_cluster = per_cluster.clamp(4, 24);
        let anchor = match parse_id_or_null(sc_track_id) {
            Some(a) => a,
            None => return Ok(ClusterBuilder::new().finish()),
        };

        let primary_artist = self.load_primary_artist_id(sc_track_id).await;

        let seed = self.load_track_vectors(anchor).await;
        let mert_seed = seed.mert.clone();
        let clap_seed = seed.clap.clone();
        let lyrics_seed = seed.lyrics.clone();
        let collab_seed = seed.collab.clone();

        let exclude: Vec<String> = vec![sc_track_id.to_string()];

        let wave_fut = smart_wave::cluster_track_ids(
            self,
            sc_user_id,
            languages,
            SmartWaveSeed::Track(anchor),
            WAVE_LIMIT,
        );

        let same_artist_fut = async {
            match primary_artist {
                Some(artist_id) => {
                    self.load_same_artist_tracks(
                        artist_id,
                        sc_track_id,
                        mert_seed.as_deref(),
                        per_cluster,
                    )
                    .await
                }
                None => Vec::new(),
            }
        };

        let same_vibe_fut = async {
            let filter = self.build_filter(&exclude, languages);
            let mert_fut = async {
                if let Some(v) = &mert_seed {
                    self.search_by_vector(
                        collections::TRACKS_MERT,
                        v,
                        filter.as_ref(),
                        SAME_VIBE_POOL,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let clap_fut = async {
                if let Some(v) = &clap_seed {
                    self.search_by_vector(
                        collections::TRACKS_CLAP,
                        v,
                        filter.as_ref(),
                        SAME_VIBE_POOL / 2,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let lyrics_fut = async {
                if let Some(v) = &lyrics_seed {
                    self.search_by_vector(
                        collections::TRACKS_LYRICS,
                        v,
                        filter.as_ref(),
                        SAME_VIBE_POOL / 2,
                    )
                    .await
                } else {
                    Vec::new()
                }
            };
            let (mert_pool, clap_pool, lyrics_pool) = tokio::join!(mert_fut, clap_fut, lyrics_fut);
            merge_audio_pools(&mert_pool, &clap_pool, &lyrics_pool)
        };

        let featured_fut = self.load_featured_with(sc_track_id, FEATURED_LIMIT);

        let fans_also_fut = async {
            match &collab_seed {
                Some(v) => {
                    let filter = self.build_filter(&exclude, languages);
                    self.search_by_vector(
                        collections::TRACKS_COLLAB,
                        v,
                        filter.as_ref(),
                        FANS_ALSO_LIMIT,
                    )
                    .await
                }
                None => Vec::new(),
            }
        };

        let (wave_ids, same_artist_ids, same_vibe_pool, featured_raw, fans_also_pool) = tokio::join!(
            wave_fut,
            same_artist_fut,
            same_vibe_fut,
            featured_fut,
            fans_also_fut,
        );

        let mut builder = ClusterBuilder::new();
        builder.reserve(std::iter::once(sc_track_id.to_string()));

        builder.push("wave", wave_ids);
        builder.push("same_artist", same_artist_ids);

        let same_vibe_artist: Option<Uuid> = primary_artist;
        let vibe_filtered = filter_vibe_pool(&same_vibe_pool, builder.taken(), same_vibe_artist);
        let vibe_ids = pick_unique_ids(&vibe_filtered, builder.taken(), per_cluster);
        builder.push("same_vibe", vibe_ids);

        let featured_filtered: Vec<ClusterNeighbor> = featured_raw
            .into_iter()
            .filter(|n| !builder.taken().contains(&n.track_id))
            .take(per_cluster)
            .collect();
        builder.push_with_neighbors("featured_with", featured_filtered);

        let fans_ids = pick_unique_ids(&fans_also_pool, builder.taken(), per_cluster);
        builder.push("fans_also", fans_ids);

        let missing = self
            .s3
            .find_missing(&builder.all_track_ids())
            .await
            .unwrap_or_default();
        builder.drop_missing(&missing);

        let result = builder.finish();
        super::impressions::log_clusters_async(
            self.pg.clone(),
            sc_user_id.to_string(),
            super::impressions::ImpressionSource::Similar,
            &result.clusters,
            &std::collections::HashMap::new(),
        );
        info!(
            track = sc_track_id,
            clusters = result.clusters.len(),
            "similar_wave built"
        );
        Ok(result)
    }

    async fn load_primary_artist_id(&self, sc_track_id: &str) -> Option<Uuid> {
        sqlx::query_as::<_, PrimaryArtistRow>(
            "SELECT primary_artist_id FROM tracks
             WHERE sc_track_id = $1 AND primary_artist_id IS NOT NULL
             LIMIT 1",
        )
        .bind(sc_track_id)
        .fetch_optional(&self.pg)
        .await
        .ok()
        .flatten()
        .map(|r| r.primary_artist_id)
    }

    async fn load_same_artist_tracks(
        &self,
        artist_id: Uuid,
        anchor_track_id: &str,
        mert_seed: Option<&[f32]>,
        limit: usize,
    ) -> Vec<String> {
        let rows: Vec<(String,)> = sqlx::query_as(
            "SELECT it.sc_track_id
             FROM track_artists ta
             JOIN tracks it ON it.id = ta.track_id
             LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
             WHERE ta.artist_id = $1
               AND ta.role = 'primary'
               AND it.sharing = 'public'
               AND it.sc_track_id <> $2
             ORDER BY COALESCE(c.play_count, 0) DESC
             LIMIT $3",
        )
        .bind(artist_id)
        .bind(anchor_track_id)
        .bind(SAME_ARTIST_POOL)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();
        let pool: Vec<String> = rows.into_iter().map(|(id,)| id).collect();
        if pool.is_empty() {
            return Vec::new();
        }

        let Some(seed) = mert_seed else {
            return pool.into_iter().take(limit).collect();
        };
        let numeric_ids: Vec<u64> = pool.iter().filter_map(|s| s.parse::<u64>().ok()).collect();
        let vec_map = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
            .await;
        let mut scored: Vec<(String, f32)> = pool
            .into_iter()
            .filter_map(|id| {
                let v = vec_map.get(&id)?;
                Some((id, cosine(seed, v)))
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(limit).map(|(id, _)| id).collect()
    }

    async fn load_featured_with(&self, anchor_track_id: &str, limit: i64) -> Vec<ClusterNeighbor> {
        let rows: Vec<ArtistTrackRow> = sqlx::query_as::<_, ArtistTrackRow>(
            "WITH anchor_artists AS (
                 SELECT artist_id FROM track_artists ta
                 JOIN tracks it ON it.id = ta.track_id
                 WHERE it.sc_track_id = $1
             ),
             feat_artists AS (
                 SELECT DISTINCT ta.artist_id
                 FROM track_artists ta
                 JOIN tracks it ON it.id = ta.track_id
                 WHERE ta.role IN ('featured', 'remixer')
                   AND it.id IN (
                       SELECT track_id FROM track_artists
                       WHERE artist_id IN (SELECT artist_id FROM anchor_artists)
                   )
                   AND ta.artist_id NOT IN (SELECT artist_id FROM anchor_artists)
             ),
             ranked AS (
                 SELECT
                     ta.artist_id, it.sc_track_id,
                     ROW_NUMBER() OVER (
                         PARTITION BY ta.artist_id
                         ORDER BY COALESCE(c.play_count, 0) DESC
                     ) AS rn
                 FROM feat_artists fa
                 JOIN track_artists ta ON ta.artist_id = fa.artist_id AND ta.role = 'primary'
                 JOIN tracks it ON it.id = ta.track_id
                 LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
                 WHERE it.sc_track_id <> $1
                   AND it.sharing = 'public'
             )
             SELECT a.id AS artist_id, a.name AS artist_name, a.avatar_url, r.sc_track_id
             FROM ranked r
             JOIN artists a ON a.id = r.artist_id
             WHERE r.rn = 1 AND a.merged_into IS NULL
             LIMIT $2",
        )
        .bind(anchor_track_id)
        .bind(limit)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();

        rows.into_iter()
            .map(|r| ClusterNeighbor {
                track_id: r.sc_track_id,
                artist_id: r.artist_id,
                artist_name: r.artist_name,
                avatar_url: r.avatar_url,
            })
            .collect()
    }
}

fn filter_vibe_pool(
    pool: &[super::service::RecommendResult],
    taken: &HashSet<String>,
    same_artist_id: Option<Uuid>,
) -> Vec<super::service::RecommendResult> {
    let same_artist_str = same_artist_id.map(|id| id.to_string());
    pool.iter()
        .filter(|r| {
            let id = super::clusters::recommend_id_str(&r.id);
            if id.is_empty() || taken.contains(&id) {
                return false;
            }
            if let (Some(same_id), Some(payload)) = (same_artist_str.as_deref(), r.payload.as_ref())
            {
                if let Some(pa) = payload.get("primary_artist_id").and_then(|v| v.as_str()) {
                    if pa == same_id {
                        return false;
                    }
                }
            }
            true
        })
        .cloned()
        .collect()
}
