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

/// Предохранитель от срыва одного HTTP/2-канала к qdrant при сотнях
/// параллельных stream'ов — НЕ лимит qdrant. Дефолт 128 (одна сборка волны
/// = 40+ запросов, чтобы 2-3 юзера не сериализовались). Тюн: `QDRANT_MAX_CONCURRENCY`.
fn qdrant_max_concurrency() -> usize {
    std::env::var("QDRANT_MAX_CONCURRENCY")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(128)
}

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
            qdrant_sem: Arc::new(Semaphore::new(qdrant_max_concurrency())),
            pg,
            redis,
            worker,
            s3,
            collab,
            cfg,
        })
    }
}
