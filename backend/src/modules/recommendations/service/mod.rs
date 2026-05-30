mod enrichment;
mod qdrant_io;
mod seed;
mod types;
pub(crate) mod util;
mod verify;

pub use types::RecommendResult;
pub(crate) use types::ScoredCandidate;

use std::sync::Arc;

use deadpool_redis::Pool as RedisPool;
use sqlx::PgPool;

use crate::config::SoundwaveCfg;
use crate::modules::collab::CollabVectorService;
use crate::modules::lyrics::WorkerClient;
use crate::modules::recommendations::s3_verifier::S3VerifierService;
use crate::qdrant::QdrantService;

pub struct RecommendationsService {
    pub(crate) qdrant: Arc<QdrantService>,
    pub(crate) pg: PgPool,
    pub(crate) redis: RedisPool,
    pub(crate) worker: Arc<WorkerClient>,
    pub(crate) s3: Arc<S3VerifierService>,
    pub(crate) collab: Arc<CollabVectorService>,
    pub(crate) cfg: SoundwaveCfg,
}

impl RecommendationsService {
    pub fn new(
        qdrant: Arc<QdrantService>,
        pg: PgPool,
        redis: RedisPool,
        worker: Arc<WorkerClient>,
        s3: Arc<S3VerifierService>,
        collab: Arc<CollabVectorService>,
        cfg: SoundwaveCfg,
    ) -> Arc<Self> {
        Arc::new(Self {
            qdrant,
            pg,
            redis,
            worker,
            s3,
            collab,
            cfg,
        })
    }
}
