mod candidates;
mod enrichment;
mod fusion;
mod qdrant_io;
mod rerank;
mod scoring;
mod seed;
mod types;
pub(crate) mod util;
mod verify;

pub(crate) use types::ScoredCandidate;
pub use types::{RecommendResult, WaveMode};

use std::sync::Arc;

use sqlx::PgPool;

use crate::config::SoundwaveCfg;
use crate::modules::centroids::CentroidService;
use crate::modules::collab::CollabVectorService;
use crate::modules::ltr::LtrService;
use crate::modules::lyrics::WorkerClient;
use crate::modules::recommendations::s3_verifier::S3VerifierService;
use crate::qdrant::QdrantService;

pub struct RecommendationsService {
    pub(crate) qdrant: Arc<QdrantService>,
    pub(crate) pg: PgPool,
    pub(crate) worker: Arc<WorkerClient>,
    pub(crate) s3: Arc<S3VerifierService>,
    pub(crate) centroids: Arc<CentroidService>,
    pub(crate) collab: Arc<CollabVectorService>,
    pub(crate) ltr: Arc<LtrService>,
    pub(crate) cfg: SoundwaveCfg,
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
}
