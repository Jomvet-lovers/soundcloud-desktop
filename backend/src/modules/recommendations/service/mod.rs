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
use tokio::sync::Semaphore;

use crate::config::SoundwaveCfg;
use crate::modules::collab::CollabVectorService;
use crate::modules::lyrics::WorkerClient;
use crate::modules::recommendations::s3_verifier::S3VerifierService;
use crate::qdrant::QdrantService;

/// Лимит одновременных gRPC-запросов к qdrant. Tonic мультиплексирует
/// stream'ы по одному HTTP/2 соединению; при сотнях параллельных recommend'ов
/// канал срывается (h2 protocol error / operation cancelled). 16 — комфортный
/// потолок без видимого замедления одного запроса волны.
const QDRANT_MAX_CONCURRENCY: usize = 16;

pub struct RecommendationsService {
    pub(crate) qdrant: Arc<QdrantService>,
    pub(crate) qdrant_sem: Arc<Semaphore>,
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
            qdrant_sem: Arc::new(Semaphore::new(QDRANT_MAX_CONCURRENCY)),
            pg,
            redis,
            worker,
            s3,
            collab,
            cfg,
        })
    }
}
