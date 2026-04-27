use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::postgres::PgPool;
use crate::stream::storage::StorageClient;

pub fn spawn_cleanup_task(config: Config, pg: PgPool, storage: Arc<StorageClient>) {
    if !storage.enabled() {
        info!("[cleanup] storage disabled, skipping cleanup task");
        return;
    }

    let interval = Duration::from_secs(config.storage_cleanup_interval_secs);
    info!(
        "[cleanup] starting (interval={}s, max_age={}d, max_size={}B)",
        config.storage_cleanup_interval_secs,
        config.storage_cleanup_days,
        config.storage_max_size_bytes
    );

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            run_cleanup(&config, &pg, &storage).await;
        }
    });
}

async fn run_cleanup(config: &Config, pg: &PgPool, storage: &StorageClient) {
    let mut deleted = 0u64;

    if config.storage_cleanup_days > 0 {
        match pg.get_stale_cdn_tracks(config.storage_cleanup_days).await {
            Ok(tracks) => {
                for track in tracks {
                    if let Some(ref path) = track.cdn_path {
                        if let Err(e) = storage.delete_file(path).await {
                            warn!("[cleanup] failed to delete storage file {path}: {e}");
                            continue;
                        }
                    }
                    if let Err(e) = pg.delete_cdn_track(&track.id).await {
                        warn!("[cleanup] failed to delete PG record {}: {e}", track.id);
                    } else {
                        deleted += 1;
                    }
                }
            }
            Err(e) => warn!("[cleanup] get stale tracks failed: {e}"),
        }
    }

    if config.storage_max_size_bytes > 0 {
        loop {
            match pg.get_cdn_tracks_oldest_first(100).await {
                Ok(tracks) if !tracks.is_empty() => {
                    for track in tracks {
                        if let Some(ref path) = track.cdn_path {
                            if let Err(e) = storage.delete_file(path).await {
                                warn!("[cleanup] size-cleanup failed to delete {path}: {e}");
                                continue;
                            }
                        }
                        let _ = pg.delete_cdn_track(&track.id).await;
                        deleted += 1;
                    }
                    break;
                }
                _ => break,
            }
        }
    }

    if deleted > 0 {
        info!("[cleanup] removed {deleted} stale storage tracks");
    }
}
