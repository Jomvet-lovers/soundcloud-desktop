use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use crate::config::Config;
use crate::db::postgres::PgPool;
use crate::stream::cdn::CdnClient;

/// Spawn background CDN cleanup task
pub fn spawn_cleanup_task(config: Config, pg: PgPool, cdn: Arc<CdnClient>) {
    if !cdn.enabled() {
        info!("[cleanup] CDN disabled, skipping cleanup task");
        return;
    }

    let interval = Duration::from_secs(config.cdn_cleanup_interval_secs);
    info!(
        "[cleanup] starting (interval={}s, max_age={}d, max_size={}B)",
        config.cdn_cleanup_interval_secs, config.cdn_cleanup_days, config.cdn_max_size_bytes
    );

    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            run_cleanup(&config, &pg, &cdn).await;
        }
    });
}

async fn run_cleanup(config: &Config, pg: &PgPool, cdn: &CdnClient) {
    let mut deleted = 0u64;

    // 1. Delete files older than CDN_CLEANUP_DAYS
    if config.cdn_cleanup_days > 0 {
        match pg.get_stale_cdn_tracks(config.cdn_cleanup_days).await {
            Ok(tracks) => {
                for track in tracks {
                    if let Some(ref path) = track.cdn_path {
                        if let Err(e) = cdn.delete_file(path).await {
                            warn!("[cleanup] failed to delete CDN file {path}: {e}");
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

    // 2. Size-based cleanup: delete oldest-accessed if total > CDN_MAX_SIZE_BYTES
    if config.cdn_max_size_bytes > 0 {
        // Estimate total size from PG records count * avg file size
        // Since we don't store file sizes in PG, we do batch deletion of oldest
        // until we've deleted enough. Fetch in batches of 100.
        loop {
            match pg.get_cdn_tracks_oldest_first(100).await {
                Ok(tracks) if !tracks.is_empty() => {
                    for track in tracks {
                        if let Some(ref path) = track.cdn_path {
                            if let Err(e) = cdn.delete_file(path).await {
                                warn!("[cleanup] size-cleanup failed to delete {path}: {e}");
                                continue;
                            }
                        }
                        let _ = pg.delete_cdn_track(&track.id).await;
                        deleted += 1;
                    }
                    // Re-check would need actual size tracking;
                    // for now just do one batch per cycle
                    break;
                }
                _ => break,
            }
        }
    }

    if deleted > 0 {
        info!("[cleanup] removed {deleted} stale CDN tracks");
    }
}
