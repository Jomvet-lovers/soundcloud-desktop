use std::collections::HashSet;
use tracing::info;

use crate::qdrant::collections;

use super::util::value_to_u64;
use super::RecommendationsService;

impl RecommendationsService {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn build_candidate_pool(
        &self,
        user_collab: Option<&[f32]>,
        seed_has_taste: bool,
        user_taste_id: u64,
        anchor: Option<u64>,
        positive_ids: &[u64],
        negative_ids: &[u64],
        exclude: &[String],
        languages: Option<&[String]>,
        fetch_limit: usize,
        req_id: &str,
    ) -> Vec<u64> {
        let filter = self.build_filter(exclude, languages);
        let mut pool: HashSet<u64> = HashSet::new();

        if let Some(uc) = user_collab {
            let res = self
                .search_by_vector(collections::TRACKS_COLLAB, uc, filter.as_ref(), fetch_limit)
                .await;
            for r in &res {
                if let Some(n) = value_to_u64(&r.id) {
                    pool.insert(n);
                }
            }
            info!(req_id, count = res.len(), "pool collab-arm");
        }

        if seed_has_taste {
            let res = self
                .recommend_by_lookup(
                    collections::TRACKS_MERT,
                    &[user_taste_id],
                    negative_ids,
                    collections::USER_TASTE_MERT,
                    filter.as_ref(),
                    fetch_limit,
                )
                .await;
            for r in &res {
                if let Some(n) = value_to_u64(&r.id) {
                    pool.insert(n);
                }
            }
            info!(req_id, count = res.len(), "pool taste-arm");
        } else if !positive_ids.is_empty() && user_collab.is_none() {
            let res = self
                .recommend_by_positive(
                    collections::TRACKS_MERT,
                    positive_ids,
                    filter.as_ref(),
                    fetch_limit,
                    negative_ids,
                )
                .await;
            for r in &res {
                if let Some(n) = value_to_u64(&r.id) {
                    pool.insert(n);
                }
            }
            info!(req_id, count = res.len(), "pool cold-start-arm");
        }

        if let Some(a) = anchor {
            let res = self
                .recommend_by_positive(
                    collections::TRACKS_MERT,
                    &[a],
                    filter.as_ref(),
                    fetch_limit,
                    negative_ids,
                )
                .await;
            for r in &res {
                if let Some(n) = value_to_u64(&r.id) {
                    if n != a {
                        pool.insert(n);
                    }
                }
            }
            info!(req_id, count = res.len(), "pool anchor-arm");
        }

        pool.into_iter().collect()
    }

    pub(crate) async fn coplay_arm(
        &self,
        sc_user_id: &str,
        anchor: Option<u64>,
        exclude: &[String],
        limit: usize,
        req_id: &str,
    ) -> Vec<u64> {
        if limit == 0 {
            return Vec::new();
        }
        let mut seed_sc_ids: Vec<String> = Vec::new();
        if let Some(a) = anchor {
            seed_sc_ids.push(a.to_string());
        }
        let recent: Vec<(String,)> = sqlx::query_as(
            "SELECT sc_track_id FROM user_events
             WHERE sc_user_id = $1
               AND event_type IN ('like', 'play_complete')
             ORDER BY created_at DESC
             LIMIT 50",
        )
        .bind(sc_user_id)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();
        for (id,) in recent {
            seed_sc_ids.push(id);
        }
        if seed_sc_ids.is_empty() {
            return Vec::new();
        }
        let co_artists: Vec<(uuid::Uuid,)> = sqlx::query_as(
            "WITH seed AS (
                 SELECT DISTINCT primary_artist_id AS aid
                 FROM indexed_tracks
                 WHERE sc_track_id = ANY($1) AND primary_artist_id IS NOT NULL
             )
             SELECT (CASE WHEN ac.a_id = s.aid THEN ac.b_id ELSE ac.a_id END) AS co_id
             FROM seed s
             JOIN artist_coplay ac ON ac.a_id = s.aid OR ac.b_id = s.aid
             GROUP BY co_id
             ORDER BY MAX(ac.weight) DESC
             LIMIT 30",
        )
        .bind(&seed_sc_ids)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();
        if co_artists.is_empty() {
            return Vec::new();
        }
        let co_ids: Vec<uuid::Uuid> = co_artists.into_iter().map(|(a,)| a).collect();

        let track_rows: Vec<(String,)> = sqlx::query_as(
            "SELECT it.sc_track_id
             FROM track_artists ta
             JOIN indexed_tracks it ON it.id = ta.indexed_track_id
             LEFT JOIN sc_track_counters c ON c.sc_track_id = it.sc_track_id
             WHERE ta.artist_id = ANY($1)
               AND ta.role = 'primary'
               AND it.indexed_at IS NOT NULL
               AND NOT (it.sc_track_id = ANY($2))
             ORDER BY COALESCE(c.play_count, 0) DESC
             LIMIT $3",
        )
        .bind(&co_ids)
        .bind(exclude)
        .bind(limit as i64)
        .fetch_all(&self.pg)
        .await
        .unwrap_or_default();

        let out: Vec<u64> = track_rows
            .into_iter()
            .filter_map(|(id,)| id.parse::<u64>().ok())
            .collect();
        info!(req_id, count = out.len(), "pool coplay-arm");
        out
    }
}
