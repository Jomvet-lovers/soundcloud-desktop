use std::sync::Arc;

use qdrant_client::config::QdrantConfig;
use qdrant_client::qdrant::{Distance, VectorParamsBuilder};
use qdrant_client::Qdrant;
use tracing::{info, warn};

use crate::config::QdrantCfg;
use crate::error::{AppError, AppResult};

pub mod collections {
    pub const TRACKS_MERT: &str = "tracks_mert";
    pub const TRACKS_CLAP: &str = "tracks_clap";
    pub const TRACKS_LYRICS: &str = "tracks_lyrics";
    pub const TRACKS_COLLAB: &str = "tracks_collab";
}

pub struct QdrantService {
    client: Qdrant,
}

impl QdrantService {
    pub fn connect(cfg: &QdrantCfg) -> AppResult<Arc<Self>> {
        let mut qcfg = QdrantConfig::from_url(&cfg.url);
        if !cfg.api_key.is_empty() {
            qcfg = qcfg.api_key(cfg.api_key.clone());
        }
        let client = Qdrant::new(qcfg)
            .map_err(|e| AppError::internal(format!("qdrant client init: {e}")))?;
        Ok(Arc::new(Self { client }))
    }

    pub fn raw(&self) -> &Qdrant {
        &self.client
    }

    pub async fn bootstrap_collections(&self) {
        let collections = match self.client.list_collections().await {
            Ok(c) => c,
            Err(e) => {
                warn!(error = %e, "Qdrant init skipped (not available)");
                return;
            }
        };
        let existing: std::collections::HashSet<String> = collections
            .collections
            .into_iter()
            .map(|c| c.name)
            .collect();

        for (name, size) in [
            (collections::TRACKS_MERT, 1024u64),
            (collections::TRACKS_CLAP, 512),
            (collections::TRACKS_LYRICS, 1024),
        ] {
            if existing.contains(name) {
                continue;
            }
            let req = qdrant_client::qdrant::CreateCollectionBuilder::new(name)
                .vectors_config(VectorParamsBuilder::new(size, Distance::Cosine))
                .on_disk_payload(true)
                .build();
            match self.client.create_collection(req).await {
                Ok(_) => info!(collection = name, size, "Qdrant collection created"),
                Err(e) => warn!(collection = name, error = %e, "Qdrant collection create failed"),
            }
        }
    }
}
