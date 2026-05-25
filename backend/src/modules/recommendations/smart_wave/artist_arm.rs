//! Artist-arm: треки из сетки артистов (artist_graph).
//!
//! Берём top-N артистов из affinity-листа, для каждого тянем самые сильные
//! треки (по play_count) и нормализуем итоговый score как
//! `artist_affinity_weight * (0.6 + 0.4 * track_play_rank)` — артист
//! доминирует над треком, но не полностью.

use std::collections::HashSet;

use sqlx::PgPool;
use tracing::debug;
use uuid::Uuid;

use super::artist_graph::ArtistAffinity;

const TOP_ARTISTS: usize = 30;
const TRACKS_PER_ARTIST: i64 = 4;

#[derive(Debug, Clone)]
pub struct ArtistArmCandidate {
    pub sc_track_id: u64,
    pub score: f32,
}

pub async fn pick_tracks(
    pg: &PgPool,
    affinity: &[ArtistAffinity],
    exclude: &HashSet<String>,
    limit: usize,
) -> Vec<ArtistArmCandidate> {
    if affinity.is_empty() {
        return Vec::new();
    }
    let top: Vec<&ArtistAffinity> = affinity.iter().take(TOP_ARTISTS).collect();
    let artist_ids: Vec<Uuid> = top.iter().map(|a| a.artist_id).collect();
    let exclude_vec: Vec<String> = exclude.iter().cloned().collect();

    let rows: Vec<(Uuid, String, i64, i64)> = match sqlx::query_as(
        "WITH ranked AS ( \
             SELECT ta.artist_id, it.sc_track_id, \
                 COALESCE(c.play_count, 0) AS play_count, \
                 ROW_NUMBER() OVER ( \
                     PARTITION BY ta.artist_id \
                     ORDER BY COALESCE(c.play_count, 0) DESC \
                 ) AS rn \
             FROM track_artists ta \
             JOIN tracks it ON it.id = ta.track_id \
             LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id \
             WHERE ta.artist_id = ANY($1) \
               AND ta.role = 'primary' \
               AND NOT (it.sc_track_id = ANY($2)) \
         ) \
         SELECT artist_id, sc_track_id, play_count, rn FROM ranked WHERE rn <= $3",
    )
    .bind(&artist_ids)
    .bind(&exclude_vec)
    .bind(TRACKS_PER_ARTIST)
    .fetch_all(pg)
    .await
    {
        Ok(v) => v,
        Err(e) => {
            debug!(error = %e, "artist_arm: query failed");
            return Vec::new();
        }
    };

    let weight_by_artist: std::collections::HashMap<Uuid, f32> =
        top.iter().map(|a| (a.artist_id, a.weight)).collect();

    let mut out: Vec<ArtistArmCandidate> = Vec::with_capacity(rows.len());
    for (artist_id, sc_track_id, _play_count, rn) in rows {
        let Ok(n) = sc_track_id.parse::<u64>() else {
            continue;
        };
        let artist_w = *weight_by_artist.get(&artist_id).unwrap_or(&0.5);
        // rn=1 → track_rank=1.0; rn=TRACKS_PER_ARTIST → ~0.0.
        let track_rank = 1.0 - ((rn - 1) as f32 / TRACKS_PER_ARTIST.max(1) as f32);
        let score = artist_w * (0.6 + 0.4 * track_rank);
        out.push(ArtistArmCandidate {
            sc_track_id: n,
            score,
        });
    }
    out.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}
