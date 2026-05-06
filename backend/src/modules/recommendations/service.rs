use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use qdrant_client::qdrant::{
    point_id::PointIdOptions, vector_output::Vector as VectorVariant,
    vectors_output::VectorsOptions, Condition, Filter, GetPointsBuilder, PointId,
    RecommendStrategy, SearchPointsBuilder, Value as QValue,
};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use tracing::{debug, info, warn};

use crate::common::user_id::user_id_to_qdrant_id;
use crate::config::SoundwaveCfg;
use crate::error::AppResult;
use crate::modules::centroids::{cosine, CentroidService};
use crate::modules::collab::CollabVectorService;
use crate::modules::ltr::{LtrService, LTR_FEATURE_COUNT};
use crate::modules::lyrics::WorkerClient;
use crate::modules::recommendations::s3_verifier::S3VerifierService;
use crate::qdrant::{collections, QdrantService};

const DIVERSE_DIVERSITY: f32 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaveMode {
    Similar,
    Diverse,
}

impl WaveMode {
    pub fn parse(raw: Option<&str>) -> Self {
        match raw {
            Some("diverse") => Self::Diverse,
            _ => Self::Similar,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RecommendResult {
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<HashMap<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artist: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genre: Option<String>,
    #[serde(rename = "playbackCount", skip_serializing_if = "Option::is_none")]
    pub playback_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub features: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Default)]
struct SeedVectors {
    collab: Option<Vec<f32>>,
    mert: Option<Vec<f32>>,
    clap: Option<Vec<f32>>,
    lyrics: Option<Vec<f32>>,
}

#[derive(Debug, Clone)]
struct ScoredCandidate {
    id: u64,
    score: f32,
    payload: Option<HashMap<String, Value>>,
    features: Vec<f32>,
}

pub struct RecommendationsService {
    qdrant: Arc<QdrantService>,
    pg: PgPool,
    worker: Arc<WorkerClient>,
    s3: Arc<S3VerifierService>,
    centroids: Arc<CentroidService>,
    collab: Arc<CollabVectorService>,
    ltr: Arc<LtrService>,
    cfg: SoundwaveCfg,
}

impl RecommendationsService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        qdrant: Arc<QdrantService>,
        pg: PgPool,
        worker: Arc<WorkerClient>,
        s3: Arc<S3VerifierService>,
        centroids: Arc<CentroidService>,
        collab: Arc<CollabVectorService>,
        ltr: Arc<LtrService>,
        cfg: SoundwaveCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            qdrant,
            pg,
            worker,
            s3,
            centroids,
            collab,
            ltr,
            cfg,
        })
    }

    pub async fn recommend(
        &self,
        sc_user_id: &str,
        positive: &[String],
        negative: &[String],
        exclude: &[String],
        limit: usize,
        languages: Option<&[String]>,
        mode: WaveMode,
        req_id: &str,
    ) -> AppResult<Vec<RecommendResult>> {
        self.wave(
            sc_user_id, None, positive, negative, exclude, limit, languages, mode, req_id,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn wave(
        &self,
        sc_user_id: &str,
        sc_track_id: Option<&str>,
        positive: &[String],
        negative: &[String],
        exclude: &[String],
        limit: usize,
        languages: Option<&[String]>,
        mode: WaveMode,
        req_id: &str,
    ) -> AppResult<Vec<RecommendResult>> {
        let anchor = sc_track_id.and_then(parse_id_or_null);
        let positive_ids: Vec<u64> = positive
            .iter()
            .filter_map(|s| parse_id_or_null(s))
            .collect();
        let negative_ids: Vec<u64> = negative
            .iter()
            .filter_map(|s| parse_id_or_null(s))
            .collect();

        let div = if mode == WaveMode::Diverse {
            DIVERSE_DIVERSITY
        } else {
            0.0
        };
        let fetch_limit: usize = if mode == WaveMode::Diverse {
            (limit * 20).max(500)
        } else {
            (limit * 12).max(300)
        };
        let threshold = (self.cfg.score_threshold as f32 - div * 0.04).max(0.0);

        let user_taste_id = user_id_to_qdrant_id(sc_user_id);
        let taste_fut = self.load_user_taste_vectors(user_taste_id);
        let collab_fut = async { self.collab.get_user_vector(sc_user_id).await };
        let (taste, user_collab) = tokio::join!(taste_fut, collab_fut);
        let user_collab = user_collab?;
        let seed = SeedVectors {
            collab: user_collab.clone(),
            mert: taste.0,
            clap: taste.1,
            lyrics: taste.2,
        };
        info!(
            req_id,
            mode = ?mode,
            anchor = ?anchor,
            pos = positive_ids.len(),
            neg = negative_ids.len(),
            excl = exclude.len(),
            limit,
            fetch_limit,
            threshold,
            "wave start"
        );

        let candidate_ids = self
            .build_candidate_pool(
                user_collab.as_deref(),
                seed.mert.is_some(),
                user_taste_id,
                anchor,
                &positive_ids,
                &negative_ids,
                exclude,
                languages,
                fetch_limit,
                req_id,
            )
            .await;
        if candidate_ids.is_empty() {
            warn!(req_id, "wave: empty pool, fallback");
            return self.get_fallback_tracks(exclude, limit, languages).await;
        }

        let scored = self.score_by_all_bases(&candidate_ids, &seed, req_id).await;
        let filtered: Vec<ScoredCandidate> = scored
            .into_iter()
            .filter(|s| s.score >= threshold)
            .collect();
        info!(req_id, scored = filtered.len(), "wave scored");

        let enriched = self.enrich_and_boost(filtered, languages).await?;
        let reranked_count = enriched.len().min(limit * 4);
        let reranked = self
            .apply_ltr_rerank(enriched, reranked_count, req_id)
            .await;
        let ranked = if div > 0.0 {
            let work = reranked.len().min(limit * 8);
            self.apply_mmr(reranked, div, work).await
        } else {
            reranked
        };
        let diverse = self.artist_cap(ranked, self.cfg.artist_cap);
        let verified = self.take_verified(diverse, limit).await?;
        if verified.len() >= 5 {
            return Ok(verified);
        }
        warn!(
            req_id,
            count = verified.len(),
            "wave too few results, fallback"
        );
        self.get_fallback_tracks(exclude, limit, languages).await
    }

    pub async fn similar(
        &self,
        sc_track_id: &str,
        exclude: &[String],
        limit: usize,
        languages: Option<&[String]>,
        diversity: f32,
        req_id: &str,
    ) -> AppResult<Vec<RecommendResult>> {
        let Some(anchor) = parse_id_or_null(sc_track_id) else {
            return Ok(Vec::new());
        };

        let div = diversity.clamp(0.0, 1.0);
        let fetch_limit: usize = if div > 0.5 {
            (limit * 18).max(240)
        } else {
            (limit * 8).max(80)
        };
        let threshold = (self.cfg.score_threshold as f32 - div * 0.04).max(0.0);

        let seed = self.load_track_vectors(anchor).await;
        if seed.collab.is_none()
            && seed.mert.is_none()
            && seed.clap.is_none()
            && seed.lyrics.is_none()
        {
            warn!(req_id, anchor, "similar: no vectors");
            return Ok(Vec::new());
        }

        info!(req_id, anchor, div, limit, fetch_limit, "similar start");

        let filter = self.build_filter(exclude, languages);
        let mut pool: HashSet<u64> = HashSet::new();

        if let Some(c) = &seed.collab {
            let res = self
                .search_by_vector(collections::TRACKS_COLLAB, c, filter.as_ref(), fetch_limit)
                .await;
            for r in res {
                if let Some(n) = value_to_u64(&r.id) {
                    if n != anchor {
                        pool.insert(n);
                    }
                }
            }
        }

        let res = self
            .recommend_by_positive(
                collections::TRACKS_MERT,
                &[anchor],
                filter.as_ref(),
                fetch_limit,
                &[],
            )
            .await;
        for r in res {
            if let Some(n) = value_to_u64(&r.id) {
                if n != anchor {
                    pool.insert(n);
                }
            }
        }

        let candidate_ids: Vec<u64> = pool.into_iter().collect();
        if candidate_ids.is_empty() {
            warn!(req_id, "similar: empty pool");
            return Ok(Vec::new());
        }

        let scored = self.score_by_all_bases(&candidate_ids, &seed, req_id).await;
        let filtered: Vec<ScoredCandidate> = scored
            .into_iter()
            .filter(|s| s.score >= threshold)
            .collect();
        let enriched = self.enrich_and_boost(filtered, languages).await?;
        let reranked_count = enriched.len().min(limit * 4);
        let reranked = self
            .apply_ltr_rerank(enriched, reranked_count, req_id)
            .await;
        let ranked = if div > 0.0 {
            let work = reranked.len().min(limit * 8);
            self.apply_mmr(reranked, div, work).await
        } else {
            reranked
        };
        let cap = if div >= 0.5 { 1 } else { self.cfg.artist_cap };
        let diverse = self.artist_cap(ranked, cap);
        self.take_verified(diverse, limit).await
    }

    pub async fn search_by_text(
        &self,
        query: &str,
        limit: usize,
        languages: Option<&[String]>,
    ) -> AppResult<Vec<RecommendResult>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let vec = match self.worker.encode_text_mulan(q).await {
            Ok(Some(v)) if !v.is_empty() => v,
            _ => return Ok(Vec::new()),
        };
        let filter = self.build_filter(&[], languages);
        let fetch_limit = (limit * 3).max(40);

        let mut builder =
            SearchPointsBuilder::new(collections::TRACKS_CLAP, vec, fetch_limit as u64)
                .with_payload(true);
        if let Some(f) = filter {
            builder = builder.filter(f);
        }
        let resp = match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r,
            Err(e) => {
                debug!(error = %e, "searchByText: qdrant search failed");
                return Ok(Vec::new());
            }
        };

        let raw: Vec<RecommendResult> = resp
            .result
            .into_iter()
            .map(|p| RecommendResult {
                id: point_id_to_value(p.id),
                score: Some(p.score),
                payload: Some(payload_to_map(p.payload)),
                artist: None,
                genre: None,
                playback_count: None,
                features: None,
            })
            .collect();

        let scored: Vec<ScoredCandidate> = raw
            .into_iter()
            .filter_map(|r| {
                let id = value_to_u64(&r.id)?;
                Some(ScoredCandidate {
                    id,
                    score: r.score.unwrap_or(0.0),
                    payload: r.payload,
                    features: vec![0.0; LTR_FEATURE_COUNT],
                })
            })
            .collect();

        let enriched = self.enrich_and_boost(scored, languages).await?;
        let diverse = self.artist_cap(enriched, self.cfg.artist_cap);
        self.take_verified(diverse, limit).await
    }

    async fn score_by_all_bases(
        &self,
        candidate_ids: &[u64],
        seed: &SeedVectors,
        req_id: &str,
    ) -> Vec<ScoredCandidate> {
        let collab_fut = async {
            if seed.collab.is_some() {
                self.collab.get_track_vectors(candidate_ids).await
            } else {
                HashMap::new()
            }
        };
        let mert_fut = async {
            if seed.mert.is_some() {
                self.retrieve_vectors(collections::TRACKS_MERT, candidate_ids)
                    .await
            } else {
                HashMap::new()
            }
        };
        let clap_fut = async {
            if seed.clap.is_some() {
                self.retrieve_vectors(collections::TRACKS_CLAP, candidate_ids)
                    .await
            } else {
                HashMap::new()
            }
        };
        let lyrics_fut = async {
            if seed.lyrics.is_some() {
                self.retrieve_vectors(collections::TRACKS_LYRICS, candidate_ids)
                    .await
            } else {
                HashMap::new()
            }
        };
        let payload_fut = async {
            self.retrieve_payloads(collections::TRACKS_MERT, candidate_ids)
                .await
        };

        let (collab_map, mert_map, clap_map, lyrics_map, payload_map) =
            tokio::join!(collab_fut, mert_fut, clap_fut, lyrics_fut, payload_fut);

        let c_mert = self.centroids.get(collections::TRACKS_MERT);
        let c_clap = self.centroids.get(collections::TRACKS_CLAP);
        let w_col = self.cfg.collab_weight as f32;
        let w_m = self.cfg.audio_weight as f32;
        let w_c = self.cfg.clap_weight as f32;
        let w_l = self.cfg.lyrics_weight as f32;

        let mut with_collab = 0usize;
        let mut out: Vec<ScoredCandidate> = Vec::with_capacity(candidate_ids.len());
        for id in candidate_ids {
            let key = id.to_string();
            let tcol = collab_map.get(&key);
            let tm = mert_map.get(&key);
            let tc = clap_map.get(&key);
            let tl = lyrics_map.get(&key);
            let s_col = match (&seed.collab, tcol) {
                (Some(s), Some(t)) => cosine(t, s),
                _ => 0.0,
            };
            let s_m = match (&seed.mert, tm) {
                (Some(s), Some(t)) => self.centroids.whitened_cosine(t, s, c_mert.as_deref()),
                _ => 0.0,
            };
            let s_c = match (&seed.clap, tc) {
                (Some(s), Some(t)) => self.centroids.whitened_cosine(t, s, c_clap.as_deref()),
                _ => 0.0,
            };
            let s_l = match (&seed.lyrics, tl) {
                (Some(s), Some(t)) => cosine(t, s),
                _ => 0.0,
            };
            if tcol.is_some() {
                with_collab += 1;
            }
            let score = w_col * s_col + w_m * s_m + w_c * s_c + w_l * s_l;
            let mut features = vec![0f32; LTR_FEATURE_COUNT];
            features[0] = s_col;
            features[1] = s_m;
            features[2] = s_c;
            features[3] = s_l;
            out.push(ScoredCandidate {
                id: *id,
                score,
                payload: payload_map.get(&key).cloned(),
                features,
            });
        }
        out.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        info!(req_id, total = out.len(), with_collab, "scored");
        out
    }

    #[allow(clippy::too_many_arguments)]
    async fn build_candidate_pool(
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

    async fn load_user_taste_vectors(
        &self,
        user_taste_id: u64,
    ) -> (Option<Vec<f32>>, Option<Vec<f32>>, Option<Vec<f32>>) {
        let m_fut = self.retrieve_vector(collections::USER_TASTE_MERT, user_taste_id);
        let c_fut = self.retrieve_vector(collections::USER_TASTE_CLAP, user_taste_id);
        let l_fut = self.retrieve_vector(collections::USER_TASTE_LYRICS, user_taste_id);
        tokio::join!(m_fut, c_fut, l_fut)
    }

    async fn load_track_vectors(&self, track_id: u64) -> SeedVectors {
        let collab_fut = async { self.collab.get_track_vector(track_id).await };
        let m_fut = self.retrieve_vector(collections::TRACKS_MERT, track_id);
        let c_fut = self.retrieve_vector(collections::TRACKS_CLAP, track_id);
        let l_fut = self.retrieve_vector(collections::TRACKS_LYRICS, track_id);
        let (collab, mert, clap, lyrics) = tokio::join!(collab_fut, m_fut, c_fut, l_fut);
        SeedVectors {
            collab,
            mert,
            clap,
            lyrics,
        }
    }

    async fn search_by_vector(
        &self,
        collection: &str,
        vector: &[f32],
        filter: Option<&Filter>,
        limit: usize,
    ) -> Vec<RecommendResult> {
        let mut builder =
            SearchPointsBuilder::new(collection, vector.to_vec(), limit as u64).with_payload(true);
        if let Some(f) = filter {
            builder = builder.filter(f.clone());
        }
        match self.qdrant.raw().search_points(builder).await {
            Ok(r) => r
                .result
                .into_iter()
                .map(|p| RecommendResult {
                    id: point_id_to_value(p.id),
                    score: Some(p.score),
                    payload: Some(payload_to_map(p.payload)),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                })
                .collect(),
            Err(e) => {
                debug!(collection, error = %e, "searchByVector failed");
                Vec::new()
            }
        }
    }

    async fn recommend_by_positive(
        &self,
        collection: &str,
        positive: &[u64],
        filter: Option<&Filter>,
        limit: usize,
        negative: &[u64],
    ) -> Vec<RecommendResult> {
        let mut req = qdrant_client::qdrant::RecommendPointsBuilder::new(
            collection.to_string(),
            limit as u64,
        )
        .with_payload(true)
        .strategy(RecommendStrategy::BestScore);
        for id in positive {
            req = req.add_positive(numeric_id(*id));
        }
        for id in negative {
            req = req.add_negative(numeric_id(*id));
        }
        if let Some(f) = filter {
            req = req.filter(f.clone());
        }
        match self.qdrant.raw().recommend(req).await {
            Ok(r) => r
                .result
                .into_iter()
                .map(|p| RecommendResult {
                    id: point_id_to_value(p.id),
                    score: Some(p.score),
                    payload: Some(payload_to_map(p.payload)),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                })
                .collect(),
            Err(e) => {
                debug!(collection, error = %e, "recommendByPositive failed");
                Vec::new()
            }
        }
    }

    async fn recommend_by_lookup(
        &self,
        collection: &str,
        positive: &[u64],
        negative: &[u64],
        lookup_from: &str,
        filter: Option<&Filter>,
        limit: usize,
    ) -> Vec<RecommendResult> {
        let mut req = qdrant_client::qdrant::RecommendPointsBuilder::new(
            collection.to_string(),
            limit as u64,
        )
        .with_payload(true)
        .strategy(RecommendStrategy::BestScore)
        .lookup_from(qdrant_client::qdrant::LookupLocation {
            collection_name: lookup_from.to_string(),
            ..Default::default()
        });
        for id in positive {
            req = req.add_positive(numeric_id(*id));
        }
        for id in negative {
            req = req.add_negative(numeric_id(*id));
        }
        if let Some(f) = filter {
            req = req.filter(f.clone());
        }
        match self.qdrant.raw().recommend(req).await {
            Ok(r) => r
                .result
                .into_iter()
                .map(|p| RecommendResult {
                    id: point_id_to_value(p.id),
                    score: Some(p.score),
                    payload: Some(payload_to_map(p.payload)),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                })
                .collect(),
            Err(e) => {
                debug!(collection, error = %e, "recommendByLookup failed");
                Vec::new()
            }
        }
    }

    async fn retrieve_vector(&self, collection: &str, id: u64) -> Option<Vec<f32>> {
        let resp = self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collection, vec![numeric_id(id)]).with_vectors(true))
            .await
            .ok()?;
        let p = resp.result.into_iter().next()?;
        match p.vectors.and_then(|v| v.vectors_options)? {
            VectorsOptions::Vector(v) => match v.into_vector() {
                VectorVariant::Dense(dense) => Some(dense.data),
                _ => None,
            },
            _ => None,
        }
    }

    async fn retrieve_vectors(&self, collection: &str, ids: &[u64]) -> HashMap<String, Vec<f32>> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let pids: Vec<PointId> = ids.iter().copied().map(numeric_id).collect();
        match self
            .qdrant
            .raw()
            .get_points(GetPointsBuilder::new(collection, pids).with_vectors(true))
            .await
        {
            Ok(r) => {
                for p in r.result {
                    let id_str = match p.id.and_then(|id| id.point_id_options) {
                        Some(PointIdOptions::Num(n)) => n.to_string(),
                        Some(PointIdOptions::Uuid(u)) => u,
                        None => continue,
                    };
                    if let Some(vectors) = p.vectors {
                        if let Some(VectorsOptions::Vector(v)) = vectors.vectors_options {
                            if let VectorVariant::Dense(dense) = v.into_vector() {
                                out.insert(id_str, dense.data);
                            }
                        }
                    }
                }
            }
            Err(e) => {
                debug!(collection, error = %e, "retrieveVectors failed");
            }
        }
        out
    }

    async fn retrieve_payloads(
        &self,
        collection: &str,
        ids: &[u64],
    ) -> HashMap<String, HashMap<String, Value>> {
        let mut out = HashMap::new();
        if ids.is_empty() {
            return out;
        }
        let pids: Vec<PointId> = ids.iter().copied().map(numeric_id).collect();
        match self
            .qdrant
            .raw()
            .get_points(
                GetPointsBuilder::new(collection, pids)
                    .with_payload(true)
                    .with_vectors(false),
            )
            .await
        {
            Ok(r) => {
                for p in r.result {
                    let id_str = match p.id.and_then(|id| id.point_id_options) {
                        Some(PointIdOptions::Num(n)) => n.to_string(),
                        Some(PointIdOptions::Uuid(u)) => u,
                        None => continue,
                    };
                    out.insert(id_str, payload_to_map(p.payload));
                }
            }
            Err(e) => {
                debug!(collection, error = %e, "retrievePayloads failed");
            }
        }
        out
    }

    async fn enrich_and_boost(
        &self,
        items: Vec<ScoredCandidate>,
        user_languages: Option<&[String]>,
    ) -> AppResult<Vec<RecommendResult>> {
        if items.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<String> = items.iter().map(|it| it.id.to_string()).collect();
        let tracks: Vec<(String, Option<Value>, Option<String>)> = sqlx::query_as(
            "SELECT sc_track_id, raw_sc_data, language FROM indexed_tracks \
             WHERE sc_track_id = ANY($1)",
        )
        .bind(&ids)
        .fetch_all(&self.pg)
        .await?;
        let by_id: HashMap<String, (Option<Value>, Option<String>)> = tracks
            .into_iter()
            .map(|(id, raw, lang)| (id, (raw, lang)))
            .collect();
        let boost = self.cfg.popularity_boost as f32;
        let user_lang_set: HashSet<String> = user_languages
            .map(|l| l.iter().cloned().collect())
            .unwrap_or_default();

        let mut out: Vec<RecommendResult> = items
            .into_iter()
            .map(|it| {
                let key = it.id.to_string();
                let entry = by_id.get(&key);
                let raw = entry
                    .and_then(|(r, _)| r.as_ref())
                    .cloned()
                    .unwrap_or(Value::Null);
                let language = entry.and_then(|(_, l)| l.clone());
                let artist_pub = raw
                    .get("publisher_metadata")
                    .and_then(|v| v.get("artist"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let artist_user = raw
                    .get("user")
                    .and_then(|v| v.get("username"))
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let artist = artist_pub.or(artist_user);
                let genre = raw.get("genre").and_then(|v| v.as_str()).map(String::from);
                let playback_count = raw
                    .get("playback_count")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(0);
                let bonus = ((playback_count.max(0) as f64).ln_1p() as f32) * boost;
                let mut features = it.features.clone();
                features[4] = (playback_count.max(0) as f64).ln_1p() as f32;
                features[5] = match language.as_deref() {
                    Some(l) if user_lang_set.contains(l) => 1.0,
                    _ => 0.0,
                };
                RecommendResult {
                    id: json!(it.id),
                    score: Some(it.score + bonus),
                    payload: it.payload,
                    artist,
                    genre,
                    playback_count: Some(playback_count),
                    features: Some(features),
                }
            })
            .collect();
        out.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(out)
    }

    async fn apply_ltr_rerank(
        &self,
        items: Vec<RecommendResult>,
        work_limit: usize,
        req_id: &str,
    ) -> Vec<RecommendResult> {
        if !self.ltr.enabled() || items.len() <= 1 {
            return items;
        }
        let work_limit = work_limit.min(items.len());
        let (head, tail) = items.split_at(work_limit);
        let head_vec: Vec<RecommendResult> = head.to_vec();
        let tail_vec: Vec<RecommendResult> = tail.to_vec();
        let features: Vec<Vec<f32>> = head_vec
            .iter()
            .map(|it| {
                it.features
                    .clone()
                    .unwrap_or_else(|| vec![0.0; LTR_FEATURE_COUNT])
            })
            .collect();
        let scores = match self.ltr.score(&features).await {
            Some(s) => s,
            None => return [head_vec, tail_vec].concat(),
        };
        let mut reranked: Vec<RecommendResult> = head_vec
            .into_iter()
            .enumerate()
            .map(|(i, mut it)| {
                it.score = Some(scores[i]);
                it
            })
            .collect();
        reranked.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        info!(req_id, count = reranked.len(), "ltr-rerank applied");
        [reranked, tail_vec].concat()
    }

    async fn apply_mmr(
        &self,
        items: Vec<RecommendResult>,
        diversity: f32,
        work_limit: usize,
    ) -> Vec<RecommendResult> {
        if items.len() <= 1 {
            return items;
        }
        let lambda = (1.0 - diversity).clamp(0.0, 1.0);

        let work_limit = work_limit.min(items.len());
        let (head, tail) = items.split_at(work_limit);
        let head_vec: Vec<RecommendResult> = head.to_vec();
        let tail_vec: Vec<RecommendResult> = tail.to_vec();

        let numeric_ids: Vec<u64> = head_vec
            .iter()
            .filter_map(|it| value_to_u64(&it.id))
            .collect();
        if numeric_ids.is_empty() {
            return [head_vec, tail_vec].concat();
        }

        let vectors = self
            .retrieve_vectors(collections::TRACKS_MERT, &numeric_ids)
            .await;
        if vectors.len() < 2 {
            return [head_vec, tail_vec].concat();
        }

        let centroid = self.centroids.get(collections::TRACKS_MERT);
        let whiten = |v: &[f32]| -> Vec<f32> {
            match &centroid {
                Some(c) => {
                    let n = v.len().min(c.len());
                    let mut out = Vec::with_capacity(n);
                    for i in 0..n {
                        out.push(v[i] - c[i]);
                    }
                    out
                }
                None => v.to_vec(),
            }
        };

        let mut pool: Vec<RecommendResult> = head_vec
            .iter()
            .filter(|it| {
                value_to_u64(&it.id)
                    .map(|n| vectors.contains_key(&n.to_string()))
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        let no_vec: Vec<RecommendResult> = head_vec
            .into_iter()
            .filter(|it| {
                value_to_u64(&it.id)
                    .map(|n| !vectors.contains_key(&n.to_string()))
                    .unwrap_or(true)
            })
            .collect();
        pool.sort_by(|a, b| {
            b.score
                .unwrap_or(0.0)
                .partial_cmp(&a.score.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut selected: Vec<RecommendResult> = vec![pool.remove(0)];
        while selected.len() < work_limit && !pool.is_empty() {
            let mut best_idx = 0usize;
            let mut best_score = f32::NEG_INFINITY;
            for (i, cand) in pool.iter().enumerate() {
                let cand_id = value_to_u64(&cand.id).unwrap();
                let cand_vec_raw = vectors.get(&cand_id.to_string()).unwrap();
                let cand_vec = whiten(cand_vec_raw);
                let mut max_sim = 0f32;
                for sel in &selected {
                    let sel_id = value_to_u64(&sel.id).unwrap();
                    let sel_vec_raw = vectors.get(&sel_id.to_string()).unwrap();
                    let sel_vec = whiten(sel_vec_raw);
                    let s = cosine(&cand_vec, &sel_vec);
                    if s > max_sim {
                        max_sim = s;
                    }
                }
                let rel = cand.score.unwrap_or(0.0);
                let mmr = lambda * rel - (1.0 - lambda) * max_sim;
                if mmr > best_score {
                    best_score = mmr;
                    best_idx = i;
                }
            }
            selected.push(pool.remove(best_idx));
        }

        [selected, no_vec, tail_vec].concat()
    }

    async fn take_verified(
        &self,
        items: Vec<RecommendResult>,
        limit: usize,
    ) -> AppResult<Vec<RecommendResult>> {
        let mut out: Vec<RecommendResult> = Vec::new();
        let batch_size = limit.max(8);
        let mut i = 0usize;
        while i < items.len() && out.len() < limit {
            let end = (i + batch_size).min(items.len());
            let slice = &items[i..end];
            let ids: Vec<String> = slice.iter().map(|s| value_id_to_string(&s.id)).collect();
            let missing = self.s3.find_missing(&ids).await?;
            for item in slice {
                if out.len() >= limit {
                    break;
                }
                if !missing.contains(&value_id_to_string(&item.id)) {
                    out.push(item.clone());
                }
            }
            i += batch_size;
        }
        Ok(out)
    }

    fn artist_cap(&self, items: Vec<RecommendResult>, cap: usize) -> Vec<RecommendResult> {
        if cap == 0 {
            return items;
        }
        let mut counts: HashMap<String, usize> = HashMap::new();
        let mut out = Vec::with_capacity(items.len());
        for it in items {
            let key = it
                .artist
                .clone()
                .unwrap_or_else(|| value_id_to_string(&it.id))
                .to_lowercase();
            let n = counts.get(&key).copied().unwrap_or(0);
            if n >= cap {
                continue;
            }
            counts.insert(key, n + 1);
            out.push(it);
        }
        out
    }

    fn build_filter(&self, exclude: &[String], languages: Option<&[String]>) -> Option<Filter> {
        let mut filter = Filter::default();
        let mut populated = false;

        if !exclude.is_empty() {
            let must_not: Vec<Condition> = exclude
                .iter()
                .map(|id| Condition::matches("sc_track_id", id.clone()))
                .collect();
            filter.must_not = must_not;
            populated = true;
        }
        if let Some(langs) = languages {
            if !langs.is_empty() {
                let must = vec![Condition::matches("language", langs.to_vec())];
                filter.must = must;
                populated = true;
            }
        }
        if populated {
            Some(filter)
        } else {
            None
        }
    }

    async fn get_fallback_tracks(
        &self,
        exclude: &[String],
        limit: usize,
        languages: Option<&[String]>,
    ) -> AppResult<Vec<RecommendResult>> {
        let limit_q = (limit * 3).max(60) as i64;
        let rows: Vec<(String,)> = if let Some(langs) = languages {
            if !langs.is_empty() {
                sqlx::query_as(
                    "SELECT sc_track_id FROM indexed_tracks \
                     WHERE indexed_at IS NOT NULL AND language = ANY($1) \
                     ORDER BY indexed_at DESC LIMIT $2",
                )
                .bind(langs)
                .bind(limit_q)
                .fetch_all(&self.pg)
                .await?
            } else {
                sqlx::query_as(
                    "SELECT sc_track_id FROM indexed_tracks \
                     WHERE indexed_at IS NOT NULL \
                     ORDER BY indexed_at DESC LIMIT $1",
                )
                .bind(limit_q)
                .fetch_all(&self.pg)
                .await?
            }
        } else {
            sqlx::query_as(
                "SELECT sc_track_id FROM indexed_tracks \
                 WHERE indexed_at IS NOT NULL \
                 ORDER BY indexed_at DESC LIMIT $1",
            )
            .bind(limit_q)
            .fetch_all(&self.pg)
            .await?
        };
        let exclude_set: HashSet<String> = exclude.iter().cloned().collect();
        Ok(rows
            .into_iter()
            .filter(|(id,)| !exclude_set.contains(id))
            .take(limit)
            .map(|(id,)| {
                let mut payload = HashMap::new();
                payload.insert("sc_track_id".into(), json!(id));
                RecommendResult {
                    id: json!(id),
                    score: None,
                    payload: Some(payload),
                    artist: None,
                    genre: None,
                    playback_count: None,
                    features: None,
                }
            })
            .collect())
    }
}

fn parse_id_or_null(raw: &str) -> Option<u64> {
    let s = raw.trim();
    let last = match s.rsplit_once(':') {
        Some((_, t)) => t,
        None => s,
    };
    if !last.bytes().all(|b| b.is_ascii_digit()) || last.is_empty() {
        return None;
    }
    last.parse::<u64>().ok()
}

fn numeric_id(id: u64) -> PointId {
    PointId {
        point_id_options: Some(PointIdOptions::Num(id)),
    }
}

fn point_id_to_value(id: Option<PointId>) -> Value {
    match id.and_then(|id| id.point_id_options) {
        Some(PointIdOptions::Num(n)) => json!(n),
        Some(PointIdOptions::Uuid(u)) => json!(u),
        None => Value::Null,
    }
}

fn value_to_u64(v: &Value) -> Option<u64> {
    if let Some(n) = v.as_u64() {
        return Some(n);
    }
    if let Some(s) = v.as_str() {
        return s.parse::<u64>().ok();
    }
    None
}

fn value_id_to_string(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(n) = v.as_u64() {
        return n.to_string();
    }
    v.to_string()
}

fn payload_to_map(p: HashMap<String, QValue>) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (k, v) in p {
        out.insert(k, qvalue_to_value(v));
    }
    out
}

fn qvalue_to_value(v: QValue) -> Value {
    use qdrant_client::qdrant::value::Kind;
    match v.kind {
        Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(b)) => Value::Bool(b),
        Some(Kind::IntegerValue(i)) => json!(i),
        Some(Kind::DoubleValue(d)) => json!(d),
        Some(Kind::StringValue(s)) => Value::String(s),
        Some(Kind::ListValue(l)) => {
            Value::Array(l.values.into_iter().map(qvalue_to_value).collect())
        }
        Some(Kind::StructValue(s)) => {
            let mut m = serde_json::Map::new();
            for (k, val) in s.fields {
                m.insert(k, qvalue_to_value(val));
            }
            Value::Object(m)
        }
        None => Value::Null,
    }
}
